use super::*;
use crate::{
    app_inspection::identity::fixture,
    tasks::{Results, Snapshot},
};
use std::fs;

fn context(home: &std::path::Path) -> Context {
    Context {
        home: home.into(),
        engine_root: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
        current_app: None,
        current_id: "io.github.rememberber.moomole".into(),
        protected_paths: vec![],
    }
}
fn source(path: &std::path::Path, id: &str, kind: &str) -> Source {
    let mut results = Results::default();
    let task = uuid::Uuid::new_v4().to_string();
    results.remember(&Snapshot {
        id: task.clone(),
        kind: "apps".into(),
        state: "succeeded".into(),
        result: Some(
            serde_json::json!([{"path":path,"bundle_id":id,"name":"Fixture","source":kind}]),
        ),
        error: None,
        result_unavailable: None,
    });
    results.uninstall_target(&task, 0).unwrap()
}

#[test]
fn plans_bind_successful_list_identity_scope_and_metadata_without_granting_execution() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let app = fixture(&home, "Fixture.app", "io.moomole.fixture");
    let ctx = context(&home);
    let source = source(&app, "io.moomole.fixture", "App");
    let plan = Plan::prepare(source.clone(), &ctx).unwrap();
    let view = plan.recheck(&ctx, crate::db::now()).unwrap();
    assert!(view.read_only);
    assert!(!view.can_execute);
    assert_eq!(view.scope, "appBundleOnly");
    assert_eq!(view.related_data, "retainAll");
    assert_eq!(view.source_task_id, source.task_id);
    assert_eq!(view.source_index, 0);
    assert_eq!(view.snapshot_sha256.len(), 64);
    assert_eq!(view.state, "requiresExecutionReview");
    assert!(plan.source_matches(&source));
    let mut other = source.clone();
    other.task_id = uuid::Uuid::new_v4().to_string();
    assert!(!plan.source_matches(&other));
    let mut protected = ctx.clone();
    protected
        .protected_paths
        .push(app.join("Contents/Info.plist").to_str().unwrap().into());
    assert!(plan.recheck(&protected, crate::db::now()).is_err());
    fs::write(app.join("Contents/new-resource"), "changed").unwrap();
    assert!(plan.recheck(&ctx, crate::db::now()).is_err());
    assert!(app.is_dir());
}

#[test]
fn managed_official_self_protected_unknown_source_background_and_wrong_identity_are_retained() {
    for mode in [
        "Homebrew",
        "unknown",
        "self",
        "protected",
        "wrongId",
        "background",
        "official",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let id = if mode == "official" {
            "com.eset.fixture"
        } else {
            "io.moomole.fixture"
        };
        let app = fixture(&home, "Fixture.app", id);
        let mut ctx = context(&home);
        if mode == "self" {
            ctx.current_id = id.into();
        }
        if mode == "protected" {
            ctx.protected_paths
                .push(home.join("Applications").to_str().unwrap().into());
        }
        if mode == "background" {
            let info = app.join("Contents/Info.plist");
            let text = fs::read_to_string(&info)
                .unwrap()
                .replace("</dict>", "<key>LSBackgroundOnly</key><true/></dict>");
            fs::write(info, text).unwrap();
        }
        let row = source(
            &app,
            if mode == "wrongId" { "another.id" } else { id },
            if matches!(mode, "Homebrew" | "unknown") {
                mode
            } else {
                "App"
            },
        );
        assert!(Plan::prepare(row, &ctx).is_err(), "{mode}");
        assert!(app.is_dir());
    }
}

#[test]
fn bounded_store_expiry_revocation_and_refresh_do_not_transfer_a_plan() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let ctx = context(&home);
    let app = fixture(&home, "Fixture.app", "io.moomole.fixture");
    let row = source(&app, "io.moomole.fixture", "App");
    let mut store = Store::default();
    let plan = Plan::prepare(row.clone(), &ctx).unwrap();
    let first = store.insert(plan.clone(), crate::db::now()).unwrap();
    let second = store
        .insert(Plan::prepare(row, &ctx).unwrap(), crate::db::now())
        .unwrap();
    assert!(store.get(&first.id, crate::db::now()).is_err());
    store.discard(&first.id);
    assert!(store.get(&second.id, crate::db::now()).is_ok());
    store.discard(&second.id);
    assert!(store.get(&second.id, crate::db::now()).is_err());
    let view = store.insert(plan.clone(), crate::db::now()).unwrap();
    assert!(store.get(&view.id, view.created_at - 1).is_err());
    assert!(plan.recheck(&ctx, plan.expires_at).is_err());
    assert!(plan.recheck(&ctx, plan.created_at - 1).is_err());
    for index in 0..MAX_PLANS {
        let app = fixture(&home, &format!("Fixture-{index}.app"), "io.moomole.fixture");
        store
            .insert(
                Plan::prepare(source(&app, "io.moomole.fixture", "App"), &ctx).unwrap(),
                crate::db::now(),
            )
            .unwrap();
    }
    let app = fixture(&home, "one-too-many.app", "io.moomole.fixture");
    assert!(store
        .insert(
            Plan::prepare(source(&app, "io.moomole.fixture", "App"), &ctx).unwrap(),
            crate::db::now()
        )
        .is_err());
    assert!(
        Store::default().get(&view.id, crate::db::now()).is_err(),
        "restart has no plan or permission"
    );
}

#[test]
fn source_must_be_a_retained_successful_apps_row_and_never_a_renderer_path() {
    for (kind, state) in [
        ("status", "succeeded"),
        ("apps", "failed"),
        ("apps", "running"),
    ] {
        let mut results = Results::default();
        let id = uuid::Uuid::new_v4().to_string();
        results.remember(&Snapshot {id:id.clone(),kind:kind.into(),state:state.into(),result:Some(serde_json::json!([{"path":"/Applications/X.app","bundle_id":"a.b","source":"App","name":"X"}])),error:None,result_unavailable:None});
        assert!(results.uninstall_target(&id, 0).is_err());
    }
    let mut results = Results::default();
    let id = uuid::Uuid::new_v4().to_string();
    results.remember(&Snapshot {
        id: id.clone(),
        kind: "apps".into(),
        state: "succeeded".into(),
        result: Some(
            serde_json::json!([{"path":"/Applications/X.app","bundle_id":"a.b","name":"X"}]),
        ),
        error: None,
        result_unavailable: None,
    });
    assert!(results.uninstall_target(&id, 0).is_err());
    assert!(results.uninstall_target(&id, 1).is_err());
}

#[test]
fn a_superseded_failed_or_discarded_check_cannot_restore_old_plans() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let ctx = context(&home);
    let app = fixture(&home, "Fixture.app", "io.moomole.fixture");
    let plan = Plan::prepare(source(&app, "io.moomole.fixture", "App"), &ctx).unwrap();
    let mut store = Store::default();
    let first = store.begin(plan.source.path(), None).unwrap();
    let second = store.begin(plan.source.path(), None).unwrap();
    store.finish_attempt(plan.source.path(), first);
    assert!(store
        .publish(first, plan.clone(), crate::db::now())
        .is_err());
    let view = store
        .publish(second, plan.clone(), crate::db::now())
        .unwrap();
    let pending = store.begin(plan.source.path(), Some(&view.id)).unwrap();
    assert!(store.get(&view.id, crate::db::now()).is_err());
    store.discard(&view.id);
    assert!(store
        .publish(pending, plan.clone(), crate::db::now())
        .is_err());
    assert!(
        store.begin(plan.source.path(), Some(&view.id)).is_err(),
        "a discarded observer cannot restart its old plan"
    );
    let fresh = store.begin(plan.source.path(), None).unwrap();
    store
        .publish(fresh, plan.clone(), crate::db::now())
        .unwrap();
    let pending = store.begin(plan.source.path(), Some(&view.id)).unwrap();
    store.finish_attempt(plan.source.path(), pending);
    assert!(store
        .publish(pending, plan.clone(), crate::db::now())
        .is_err());
    assert!(store.get(&view.id, crate::db::now()).is_err());
}
