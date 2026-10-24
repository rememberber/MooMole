use super::*;
use crate::uninstall_runtime::{fixture_observation, Code, Fixture};
use crate::uninstall_support::fixture_observation as support;

fn issued(store: &mut Confirmations, f: &Fixture) -> Confirmation {
    let o = fixture_observation(&f.plan, Code::NotObserved);
    let generation = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
    store
        .publish(
            generation,
            &f.plan,
            &f.ctx,
            &o,
            &support(&f.plan, &f.ctx),
            crate::db::now(),
        )
        .unwrap()
        .unwrap()
}

#[test]
fn draft_binds_whole_app_scope_and_is_consumable_once_without_execution_authority() {
    let f = Fixture::new(true);
    let mut store = Confirmations::default();
    let c = issued(&mut store, &f);
    assert_eq!(c.plan_id, f.plan.id());
    assert_eq!(c.path, f.app.to_str().unwrap());
    assert_eq!(
        (c.method, c.scope, c.related_data),
        ("appBundleTrash", "appBundleOnly", "retainAll")
    );
    assert!(c.can_execute);
    assert_eq!(c.version, 2);
    assert!(c.requires_ordinary_app_confirmation);
    assert!(c.expires_at <= f.plan.bundle_binding().3);
    assert_eq!(c.expires_at - c.created_at, 30_000);
    let selected = store
        .take(&c.id, &f.plan, &f.ctx, crate::db::now())
        .unwrap();
    assert_eq!(selected.plan_id(), f.plan.id());
    selected
        .validate(&f.plan, &f.ctx, crate::db::now())
        .unwrap();
    assert!(store
        .take(&c.id, &f.plan, &f.ctx, crate::db::now())
        .is_err());
    assert!(f.app.is_dir());
    assert!(f.home.join("app.sqlite").symlink_metadata().is_err());
}

#[test]
fn expiry_monotonic_age_rollback_and_context_changes_consume_or_withdraw_the_draft() {
    let f = Fixture::new(true);
    let mut store = Confirmations::default();
    for mode in [
        "expired",
        "rollback",
        "monotonic",
        "protected",
        "home",
        "clientId",
        "clientPath",
        "plan",
    ] {
        let c = issued(&mut store, &f);
        let mut ctx = f.ctx.clone();
        match mode {
            "protected" => ctx.protected_paths.push(f.app.to_str().unwrap().into()),
            "home" => ctx.home = f.home.join("changed-home"),
            "clientId" => ctx.current_id = "changed.id".into(),
            "clientPath" => ctx.current_app = Some(f.home.join("changed-client.app")),
            "monotonic" => {
                store.entries.get_mut(&c.id).unwrap().issued =
                    Instant::now() - Duration::from_secs(31)
            }
            _ => {}
        }
        let plan = if mode == "plan" {
            Fixture::plan(&f.app, &f.ctx)
        } else {
            f.plan.clone()
        };
        let now = match mode {
            "expired" => c.expires_at,
            "rollback" => c.created_at - 1,
            _ => crate::db::now(),
        };
        assert!(store.take(&c.id, &plan, &ctx, now).is_err(), "{mode}");
        assert!(
            store
                .take(&c.id, &f.plan, &f.ctx, crate::db::now())
                .is_err(),
            "{mode}"
        );
    }
}

#[test]
fn unknown_busy_unsupported_other_plan_or_stale_runtime_cannot_prepare_a_draft() {
    let f = Fixture::new(true);
    let mut store = Confirmations::default();
    for code in [Code::Unknown, Code::Observed, Code::Unsupported] {
        let old = issued(&mut store, &f);
        let generation = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
        let o = fixture_observation(&f.plan, code);
        assert!(store
            .publish(
                generation,
                &f.plan,
                &f.ctx,
                &o,
                &support(&f.plan, &f.ctx),
                crate::db::now()
            )
            .unwrap()
            .is_none());
        assert!(store
            .take(&old.id, &f.plan, &f.ctx, crate::db::now())
            .is_err());
    }
    let o = fixture_observation(&f.plan, Code::NotObserved);
    assert!(store
        .issue(
            &f.plan,
            &f.ctx,
            &o,
            &support(&f.plan, &f.ctx),
            crate::db::now() + 5001
        )
        .is_err());
    let other = Fixture::plan(&f.app, &f.ctx);
    assert!(store
        .issue(
            &other,
            &f.ctx,
            &o,
            &support(&other, &f.ctx),
            crate::db::now()
        )
        .is_err());
    assert!(store.entries.is_empty());
}

#[test]
fn revocation_replacement_and_superseded_workers_never_revive_older_drafts() {
    let f = Fixture::new(true);
    let mut store = Confirmations::default();
    let old = issued(&mut store, &f);
    let a = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
    let b = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
    let o = fixture_observation(&f.plan, Code::NotObserved);
    assert!(store
        .publish(
            a,
            &f.plan,
            &f.ctx,
            &o,
            &support(&f.plan, &f.ctx),
            crate::db::now()
        )
        .is_err());
    store.finish_review(f.plan.id(), a);
    let current = store
        .publish(
            b,
            &f.plan,
            &f.ctx,
            &o,
            &support(&f.plan, &f.ctx),
            crate::db::now(),
        )
        .unwrap()
        .unwrap();
    assert!(store
        .take(&old.id, &f.plan, &f.ctx, crate::db::now())
        .is_err());
    store.discard(&old.id);
    assert!(store.entries.contains_key(&current.id));
    store.revoke_plan(f.plan.id());
    assert!(store
        .take(&current.id, &f.plan, &f.ctx, crate::db::now())
        .is_err());
    let by_path = issued(&mut store, &f);
    store.revoke_path(f.app.to_str().unwrap());
    assert!(store
        .take(&by_path.id, &f.plan, &f.ctx, crate::db::now())
        .is_err());
    let pending = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
    store.revoke_plan(f.plan.id());
    assert!(store
        .publish(
            pending,
            &f.plan,
            &f.ctx,
            &o,
            &support(&f.plan, &f.ctx),
            crate::db::now()
        )
        .is_err());

    let old = issued(&mut store, &f);
    let next = Fixture::plan(&f.app, &f.ctx);
    let o = fixture_observation(&next, Code::NotObserved);
    let g = store.begin_review(next.id(), crate::db::now()).unwrap();
    store
        .publish(
            g,
            &next,
            &f.ctx,
            &o,
            &support(&next, &f.ctx),
            crate::db::now(),
        )
        .unwrap();
    assert!(store
        .take(&old.id, &f.plan, &f.ctx, crate::db::now())
        .is_err());
}

#[test]
fn bounded_drafts_and_unknown_late_results_do_not_replace_newer_observations() {
    let mut store = Confirmations::default();
    let fixtures: Vec<_> = (0..9).map(|_| Fixture::new(true)).collect();
    for f in &fixtures[..8] {
        issued(&mut store, f);
    }
    let f = &fixtures[8];
    let o = fixture_observation(&f.plan, Code::NotObserved);
    assert!(store
        .issue(
            &f.plan,
            &f.ctx,
            &o,
            &support(&f.plan, &f.ctx),
            crate::db::now()
        )
        .is_err());
    assert_eq!(store.entries.len(), 8);
    let f = &fixtures[0];
    let old = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
    let latest = store.begin_review(f.plan.id(), crate::db::now()).unwrap();
    let unknown = fixture_observation(&f.plan, Code::Unknown);
    assert!(store
        .publish(
            old,
            &f.plan,
            &f.ctx,
            &unknown,
            &support(&f.plan, &f.ctx),
            crate::db::now()
        )
        .is_err());
    let positive = fixture_observation(&f.plan, Code::NotObserved);
    assert!(store
        .publish(
            latest,
            &f.plan,
            &f.ctx,
            &positive,
            &support(&f.plan, &f.ctx),
            crate::db::now()
        )
        .unwrap()
        .is_some());
}
