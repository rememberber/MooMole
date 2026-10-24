use super::*;
use crate::{app_inspection::identity, uninstall_plan::Source};
use std::{
    collections::VecDeque,
    fs,
    process::{Child, Command},
};

pub(crate) struct Fixture {
    pub _temp: tempfile::TempDir,
    pub home: PathBuf,
    pub ctx: Context,
    pub app: PathBuf,
    pub plan: Plan,
}
impl Fixture {
    pub(crate) fn new(native: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let app = identity::fixture(&home, "运行 ' # 中文.app", "io.moomole.runtime.fixture");
        if native {
            fs::copy("/bin/sleep", app.join("Contents/MacOS/fixture-main")).unwrap();
        }
        let ctx = Context {
            home: home.clone(),
            engine_root: Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
            current_app: None,
            current_id: "io.github.rememberber.moomole".into(),
            protected_paths: vec![],
        };
        let plan = Self::plan(&app, &ctx);
        Self {
            _temp: temp,
            home,
            ctx,
            app,
            plan,
        }
    }
    pub(crate) fn plan(app: &Path, ctx: &Context) -> Plan {
        let source=Source::from_row(&uuid::Uuid::new_v4().to_string(),0,&serde_json::json!({"name":"运行 App","path":app,"bundle_id":"io.moomole.runtime.fixture","source":"App"})).unwrap();
        Plan::prepare(source, ctx).unwrap()
    }
}

struct Fake {
    lists: VecDeque<Result<BTreeSet<i32>, &'static str>>,
    paths: std::collections::HashMap<i32, Result<PathBuf, &'static str>>,
}
impl Fake {
    fn new(lists: &[&[i32]], paths: &[(i32, Result<&str, &'static str>)]) -> Self {
        Self {
            lists: lists
                .iter()
                .map(|ids| Ok(ids.iter().copied().collect()))
                .collect(),
            paths: paths
                .iter()
                .map(|(p, v)| (*p, v.map(PathBuf::from)))
                .collect(),
        }
    }
}
impl Processes for Fake {
    fn pids(&mut self) -> Result<BTreeSet<i32>, &'static str> {
        self.lists.pop_front().unwrap_or(Err("noFixtureList"))
    }
    fn executable(&mut self, pid: i32) -> Result<PathBuf, &'static str> {
        self.paths
            .get(&pid)
            .cloned()
            .unwrap_or(Err("fixtureUnreadable"))
    }
}

#[test]
fn native_path_frames_are_bounded_exact_and_keep_raw_names() {
    for good in [
        b"/a/app\0".as_slice(),
        b"/a/\xff\0".as_slice(),
        b"/a/line\nbreak\0".as_slice(),
    ] {
        let mut buf = vec![0; 4096];
        buf[..good.len()].copy_from_slice(good);
        for n in [good.len() - 1, good.len()] {
            assert!(decode_path(&buf, n as i32).is_ok());
        }
    }
    for (raw, n) in [
        (b"/a\0".as_slice(), 0),
        (b"/a\0".as_slice(), 5),
        (b"a\0".as_slice(), 1),
        (b"/a/../b\0".as_slice(), 7),
        (b"/a/./b\0".as_slice(), 6),
        (b"/abc".as_slice(), 3),
    ] {
        assert!(decode_path(raw, n).is_err());
    }
}

#[test]
fn process_matches_use_bundle_boundary_not_shared_name_and_keep_positive_on_incomplete_view() {
    let path = Path::new("/Applications/Fixture.app");
    let lists = &[&[1, 2, 3, 4][..], &[1, 2, 3, 4][..]];
    let mut source = Fake::new(
        lists,
        &[
            (1, Ok("/sbin/launchd")),
            (
                2,
                Ok("/Applications/Fixture.app.evil/Contents/MacOS/helper"),
            ),
            (3, Ok("/Applications/Other.app/Contents/MacOS/Electron")),
            (4, Ok("/usr/bin/tool")),
        ],
    );
    assert_eq!(
        sample(&mut source, path, Instant::now()).code,
        Code::NotObserved
    );
    let mut source=Fake::new(lists,&[(1,Ok("/sbin/launchd")),(2,Err("fixturePermissionDenied")),(3,Ok("/Applications/Fixture.app/Contents/Frameworks/Helper.app/Contents/MacOS/Electron")),(4,Ok("/usr/bin/tool"))]);
    let result = sample(&mut source, path, Instant::now());
    assert_eq!(result.code, Code::Observed);
    assert_eq!(result.pids, [3]);
    assert!(result.issue.is_some());
}

#[test]
fn failed_partial_changing_or_timed_out_views_never_become_absence() {
    let path = Path::new("/Applications/Fixture.app");
    let mut unavailable = Fake::new(&[], &[]);
    assert_eq!(
        sample(&mut unavailable, path, Instant::now()).code,
        Code::Unknown
    );
    let mut unreadable = Fake::new(
        &[&[1, 2], &[1, 2], &[1, 2], &[1, 2]],
        &[(1, Ok("/sbin/launchd")), (2, Err("fixtureUnreadable"))],
    );
    assert_eq!(
        sample(&mut unreadable, path, Instant::now()).code,
        Code::Unknown
    );
    let mut churn = Fake::new(
        &[&[1], &[1, 2], &[1, 2], &[1, 2, 3]],
        &[(1, Ok("/sbin/launchd")), (2, Ok("/usr/bin/tool"))],
    );
    assert_eq!(sample(&mut churn, path, Instant::now()).code, Code::Unknown);
    let mut retried = Fake::new(
        &[&[1], &[1, 2], &[1, 2], &[1, 2]],
        &[(1, Ok("/sbin/launchd")), (2, Ok("/usr/bin/tool"))],
    );
    assert_eq!(
        sample(&mut retried, path, Instant::now()).code,
        Code::NotObserved
    );
    let mut unused = Fake::new(&[], &[]);
    assert_eq!(
        sample(&mut unused, path, Instant::now() - Duration::from_secs(2)).code,
        Code::Unknown
    );
    let mut limited = Fake {
        lists: VecDeque::from([Ok((1..=MAX_PIDS as i32).collect())]),
        paths: Default::default(),
    };
    assert_eq!(
        sample(&mut limited, path, Instant::now()).code,
        Code::Unknown
    );
}

#[test]
fn observations_bind_the_original_plan_and_original_monotonic_age() {
    let f = Fixture::new(true);
    let mut o = fixture_observation(&f.plan, Code::NotObserved);
    o.permits_confirmation(&f.plan, crate::db::now()).unwrap();
    assert!(o
        .permits_confirmation(&f.plan, o.report.expires_at)
        .is_err());
    let other = Fixture::plan(&f.app, &f.ctx);
    assert!(o.permits_confirmation(&other, crate::db::now()).is_err());
    o.started = Instant::now() - Duration::from_secs(6);
    assert!(o.permits_confirmation(&f.plan, crate::db::now()).is_err());
    assert!(!o.report.can_execute);
}

#[test]
fn scripts_short_files_and_source_changes_do_not_supply_confirmation_prerequisites() {
    let f = Fixture::new(false);
    assert!(!macho_entrypoint(&f.plan).unwrap());
    let observed = observe(&f.plan, &f.ctx).unwrap();
    assert_eq!(observed.report.code, Code::Unsupported);
    assert!(observed
        .permits_confirmation(&f.plan, crate::db::now())
        .is_err());
    let native = Fixture::new(true);
    assert!(macho_entrypoint(&native.plan).unwrap());
    fs::write(
        native.app.join("Contents/MacOS/fixture-main"),
        b"changed executable",
    )
    .unwrap();
    assert!(macho_entrypoint(&native.plan).is_err());
    assert!(observe(&native.plan, &native.ctx).is_err());
}

struct ChildGuard(Child, bool);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.1 {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn alternate_root_spelling_uses_physical_identity_and_different_copy_is_not_a_match() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new(true);
    let node = f.plan.bundle_binding().1.root.nodes.last().unwrap();
    let root = Some((node.dev, node.ino));
    symlink(f.app.parent().unwrap(), f.home.join("alias-parent")).unwrap();
    let alias = f
        .home
        .join("alias-parent")
        .join(f.app.file_name().unwrap())
        .join("Contents/MacOS/fixture-main");
    assert_eq!(contains_executable(&alias, &f.app, root), Ok(true));
    fs::create_dir(f.home.join("other")).unwrap();
    let other = identity::fixture(
        &f.home.join("other"),
        f.app.file_name().unwrap().to_str().unwrap(),
        "io.moomole.runtime.fixture",
    );
    assert_eq!(
        contains_executable(&other.join("Contents/MacOS/fixture-main"), &f.app, root),
        Ok(false)
    );
    fs::remove_dir_all(&other).unwrap();
    assert_eq!(
        contains_executable(&other.join("Contents/MacOS/fixture-main"), &f.app, root),
        Err("bundleAliasUnknown")
    );
}

#[test]
fn real_native_process_path_observes_owned_bundle_helper_and_never_terminates_it() {
    let f = Fixture::new(true);
    let helper = f
        .app
        .join("Contents/Frameworks/Owned Helper.app/Contents/MacOS/helper");
    fs::create_dir_all(helper.parent().unwrap()).unwrap();
    fs::copy("/bin/sleep", &helper).unwrap();
    let plan = Fixture::plan(&f.app, &f.ctx);
    let mut child = ChildGuard(Command::new(&helper).arg("30").spawn().unwrap(), false);
    let pid = child.0.id() as i32;
    // Direct system path identity is the assertion; other process churn can
    // keep an empty view unknown and is not asserted to prove global idleness.
    let path = Native.executable(pid).unwrap();
    assert_eq!(path, helper);
    let observed = observe(&plan, &f.ctx).unwrap();
    assert_eq!(observed.report.code, Code::Observed);
    assert!(observed.report.pids.contains(&pid));
    assert!(child.0.try_wait().unwrap().is_none());
    assert!(!observed.report.can_execute);
    assert!(observed
        .permits_confirmation(&plan, crate::db::now())
        .is_err());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    child.1 = true;
    let after = observe(&plan, &f.ctx).unwrap();
    assert_ne!(after.report.code, Code::Observed);
    assert!(!after.report.can_execute);
    eprintln!("Owned native helper observed exactly; after owned child exit: {:?}; no idle/service/management authority inferred",after.report.code);
}
