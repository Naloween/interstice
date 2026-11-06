//! Views in the kernel (DESIGN §7.5): reading them, deriving their events
//! for each subscriber, and the caches of function views.

use crate::{
    logger::{LogLevel, LogSource},
    runtime::{Runtime, event::{EventInstance, SubscriptionTarget}, module::Module},
};
use interstice_abi::{
    CallVisibility, IntersticeValue, ModuleSchema, Row, SubscriptionEventSchema, ViewKind, ViewSchema, Viewer,
};
use std::{collections::BTreeMap, sync::Arc};

/// Function-view results by (owner, view, viewer); rows by encoded key.
pub(crate) type ViewCache = parking_lot::Mutex<std::collections::HashMap<(String, String, Viewer), BTreeMap<Vec<u8>, Row>>>;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Insert,
    Update,
    Delete,
}

enum Who {
    Local(String),
    Remote(crate::node::NodeId),
}

/// One subscriber of a view: who, as which viewer, and which events it
/// wants (with the reducer, for a local one).
struct Subscriber {
    who: Who,
    viewer: Viewer,
    wants: Vec<(Kind, String)>,
}

fn kind_of(event: &SubscriptionEventSchema, module: &str, view: &str) -> Option<Kind> {
    let (kind, m, t) = match event {
        SubscriptionEventSchema::Insert { module_name, table_name, .. } => (Kind::Insert, module_name, table_name),
        SubscriptionEventSchema::Update { module_name, table_name, .. } => (Kind::Update, module_name, table_name),
        SubscriptionEventSchema::Delete { module_name, table_name, .. } => (Kind::Delete, module_name, table_name),
        _ => return None,
    };
    (m == module && t == view).then_some(kind)
}

fn key_bytes(row: &Row) -> Vec<u8> {
    interstice_abi::encode(&row.primary_key).unwrap_or_default()
}

fn event(owner: &str, view: &str, kind: Kind, old: Option<Row>, new: Option<Row>) -> EventInstance {
    let (module_name, table_name) = (owner.to_string(), view.to_string());
    match kind {
        Kind::Insert => EventInstance::TableInsertEvent {
            source_node_id: None,
            module_name,
            table_name,
            inserted_row: new.expect("insert has a row"),
        },
        Kind::Update => EventInstance::TableUpdateEvent {
            source_node_id: None,
            module_name,
            table_name,
            old_row: old.expect("update has an old row"),
            new_row: new.expect("update has a new row"),
        },
        Kind::Delete => EventInstance::TableDeleteEvent {
            source_node_id: None,
            module_name,
            table_name,
            deleted_row: old.expect("delete has a row"),
        },
    }
}

impl Runtime {
    pub(crate) fn local_viewer(&self, module: &str) -> Viewer {
        Viewer { node: self.node_id.to_string(), module: module.to_string() }
    }

    pub(crate) fn remote_viewer(peer: crate::node::NodeId) -> Viewer {
        Viewer { node: peer.to_string(), module: String::new() }
    }

    /// The rows of `view` for `viewer`. `in_frame`: read from inside a call,
    /// so the caller's pending writes count (filter views on its own table).
    pub(crate) fn view_rows(
        &self,
        owner: &Arc<Module>,
        view: &ViewSchema,
        viewer: &Viewer,
        in_frame: bool,
    ) -> Result<Vec<Row>, String> {
        match &view.kind {
            ViewKind::Filter { table, predicate } => {
                let schema = owner
                    .schema
                    .tables
                    .iter()
                    .find(|t| &t.name == table)
                    .ok_or_else(|| format!("view '{}' lost its table '{table}'", view.name))?;
                let tables = owner.tables.read_recursive();
                let base = tables.get(table).ok_or_else(|| "Table not found".to_string())?;
                let rows = if in_frame {
                    Self::frame_scan(&owner.schema.name, table, base)?
                } else {
                    base.scan().to_vec()
                };
                drop(tables);
                Ok(rows
                    .into_iter()
                    .filter(|r| predicate.holds(r, &schema.primary_key.name, &schema.fields, viewer))
                    .collect())
            }
            ViewKind::Function { query, .. } => {
                let args = IntersticeValue::Vec(vec![viewer.node.clone().into(), viewer.module.clone().into()]);
                let value = self
                    .call_query(&owner.schema.name, query, args, self.node_id, "")
                    .map_err(|e| e.to_string())?;
                let items = match value {
                    IntersticeValue::Vec(items) => items,
                    other => return Err(format!("view '{}' returned {other:?}, not rows", view.name)),
                };
                items
                    .into_iter()
                    .map(|item| {
                        interstice_abi::struct_to_row(item, &view.primary_key)
                            .ok_or_else(|| format!("view '{}' returned a row without its key", view.name))
                    })
                    .collect()
            }
        }
    }

    /// Who may read `view` of `schema` from module `reader` on this node.
    pub(crate) fn may_read_view_locally(schema: &ModuleSchema, view: &str, reader: &str) -> bool {
        reader == schema.name || schema.view_visibility(view) != Some(CallVisibility::Internal)
    }

    /// Every subscriber of `owner.view`, local and remote.
    fn view_subscribers(&self, owner_schema: &ModuleSchema, view: &str) -> Vec<Subscriber> {
        let owner = owner_schema.name.as_str();
        let mut out = Vec::new();
        for module in self.modules.lock().values() {
            let wants: Vec<(Kind, String)> = module
                .schema
                .subscriptions
                .iter()
                .filter_map(|s| kind_of(&s.event, owner, view).map(|k| (k, s.reducer_name.clone())))
                .collect();
            if wants.is_empty() || !Self::may_read_view_locally(owner_schema, view, &module.schema.name) {
                continue;
            }
            out.push(Subscriber {
                who: Who::Local(module.schema.name.clone()),
                viewer: self.local_viewer(&module.schema.name),
                wants,
            });
        }
        for (peer, subscriptions) in self.node_subscriptions.lock().iter() {
            let wants: Vec<(Kind, String)> = subscriptions
                .iter()
                .filter_map(|s| kind_of(s, owner, view).map(|k| (k, String::new())))
                .collect();
            if !wants.is_empty() {
                out.push(Subscriber { who: Who::Remote(*peer), viewer: Self::remote_viewer(*peer), wants });
            }
        }
        out
    }

    fn deliver(sub: &Subscriber, kind: Kind, ev: EventInstance, out: &mut Vec<(SubscriptionTarget, EventInstance)>) {
        for (wanted, reducer) in &sub.wants {
            if *wanted != kind {
                continue;
            }
            let target = match &sub.who {
                Who::Local(module) => SubscriptionTarget::Local { module: module.clone(), reducer: reducer.clone() },
                Who::Remote(peer) => SubscriptionTarget::Remote(*peer),
            };
            out.push((target, ev.clone()));
        }
    }

    /// The events of the filter views over the table a local commit changed,
    /// for each of their subscribers (DESIGN §7.5).
    pub(crate) fn view_events(&self, event: &EventInstance) -> Vec<(SubscriptionTarget, EventInstance)> {
        let (module, table, old, new) = match event {
            EventInstance::TableInsertEvent { source_node_id: None, module_name, table_name, inserted_row } => {
                (module_name, table_name, None, Some(inserted_row))
            }
            EventInstance::TableUpdateEvent { source_node_id: None, module_name, table_name, old_row, new_row } => {
                (module_name, table_name, Some(old_row), Some(new_row))
            }
            EventInstance::TableDeleteEvent { source_node_id: None, module_name, table_name, deleted_row } => {
                (module_name, table_name, Some(deleted_row), None)
            }
            _ => return Vec::new(),
        };
        let Some(owner) = self.modules.lock().get(module).cloned() else { return Vec::new() };
        let Some(table_schema) = owner.schema.tables.iter().find(|t| &t.name == table) else { return Vec::new() };
        let (pk, fields) = (&table_schema.primary_key.name, &table_schema.fields);
        let mut out = Vec::new();
        for view in &owner.schema.views {
            let ViewKind::Filter { table: base, predicate } = &view.kind else { continue };
            if base != table {
                continue;
            }
            for sub in self.view_subscribers(&owner.schema, &view.name) {
                let holds = |r: Option<&Row>| r.is_some_and(|r| predicate.holds(r, pk, fields, &sub.viewer));
                let change = match (holds(old), holds(new)) {
                    (true, true) => Some(Kind::Update),
                    (false, true) => Some(Kind::Insert),
                    (true, false) => Some(Kind::Delete),
                    (false, false) => None,
                };
                let Some(kind) = change else { continue };
                let (o, n) = match kind {
                    Kind::Insert => (None, new.cloned()),
                    Kind::Update => (old.cloned(), new.cloned()),
                    Kind::Delete => (old.cloned(), None),
                };
                Self::deliver(&sub, kind, event_for(&owner.schema.name, &view.name, kind, o, n), &mut out);
            }
        }
        out
    }

    /// Function views reading `module.table`, as (owner, view).
    fn function_views_reading(&self, module: &str, table: &str) -> Vec<(Arc<Module>, ViewSchema)> {
        let mut out = Vec::new();
        for owner in self.modules.lock().values() {
            for view in &owner.schema.views {
                let ViewKind::Function { reads, .. } = &view.kind else { continue };
                let reads_it = reads.iter().any(|r| {
                    let m = match &r.module_selection {
                        interstice_abi::ModuleSelection::Current => owner.schema.name.as_str(),
                        interstice_abi::ModuleSelection::Other(m) => m.as_str(),
                    };
                    r.node_selection == interstice_abi::NodeSelection::Current
                        && m.eq_ignore_ascii_case(module)
                        && r.table_name.eq_ignore_ascii_case(table)
                });
                if reads_it {
                    out.push((owner.clone(), view.clone()));
                }
            }
        }
        out
    }

    /// The viewer a function view's cache is kept for.
    fn cache_viewer(view: &ViewSchema, viewer: &Viewer) -> Viewer {
        if view.per_viewer { viewer.clone() } else { Viewer { node: String::new(), module: String::new() } }
    }

    /// Re-run, diff and publish the function views reading the table a
    /// local commit changed. Runs off the event loop.
    pub(crate) fn refresh_function_views(runtime: &Arc<Self>, event: &EventInstance) {
        let (module, table) = match event {
            EventInstance::TableInsertEvent { source_node_id: None, module_name, table_name, .. }
            | EventInstance::TableUpdateEvent { source_node_id: None, module_name, table_name, .. }
            | EventInstance::TableDeleteEvent { source_node_id: None, module_name, table_name, .. } => {
                (module_name.clone(), table_name.clone())
            }
            _ => return,
        };
        let touched = runtime.function_views_reading(&module, &table);
        if touched.is_empty() {
            return;
        }
        let runtime = runtime.clone();
        tokio::task::spawn_blocking(move || {
            for (owner, view) in touched {
                let subscribers = runtime.view_subscribers(&owner.schema, &view.name);
                // One run per cached viewer: shared by all for an anonymous view.
                let mut by_viewer: BTreeMap<Viewer, Vec<&Subscriber>> = BTreeMap::new();
                for sub in &subscribers {
                    by_viewer.entry(Self::cache_viewer(&view, &sub.viewer)).or_default().push(sub);
                }
                for (viewer, subs) in by_viewer {
                    let changes = match runtime.rerun_view(&owner, &view, &viewer) {
                        Ok(changes) => changes,
                        Err(err) => {
                            runtime.logger.log(
                                &format!("View '{}.{}' failed: {err}", owner.schema.name, view.name),
                                LogSource::Runtime,
                                LogLevel::Error,
                            );
                            continue;
                        }
                    };
                    let mut out = Vec::new();
                    for (kind, old, new) in changes {
                        let ev = event_for(&owner.schema.name, &view.name, kind, old, new);
                        for sub in &subs {
                            Self::deliver(sub, kind, ev.clone(), &mut out);
                        }
                    }
                    for (target, event) in out {
                        let _ = runtime
                            .event_sender
                            .send((EventInstance::Targeted { target, event: Box::new(event) }, None));
                    }
                }
            }
        });
    }

    /// Run `view` for `viewer`, replace its cache, and return the changes.
    fn rerun_view(
        &self,
        owner: &Arc<Module>,
        view: &ViewSchema,
        viewer: &Viewer,
    ) -> Result<Vec<(Kind, Option<Row>, Option<Row>)>, String> {
        let rows = self.view_rows(owner, view, viewer, false)?;
        let new: BTreeMap<Vec<u8>, Row> = rows.into_iter().map(|r| (key_bytes(&r), r)).collect();
        let key = (owner.schema.name.clone(), view.name.clone(), viewer.clone());
        let old = self.view_cache.lock().insert(key, new.clone()).unwrap_or_default();
        let mut changes = Vec::new();
        for (k, row) in &new {
            match old.get(k) {
                None => changes.push((Kind::Insert, None, Some(row.clone()))),
                Some(before) if before != row => changes.push((Kind::Update, Some(before.clone()), Some(row.clone()))),
                Some(_) => {}
            }
        }
        for (k, row) in &old {
            if !new.contains_key(k) {
                changes.push((Kind::Delete, Some(row.clone()), None));
            }
        }
        Ok(changes)
    }

    /// Forget the caches of `module`'s views and of its subscriptions.
    pub(crate) fn drop_view_caches(&self, module: &str) {
        self.view_cache.lock().retain(|(owner, _, viewer), _| owner != module && viewer.module != module);
    }

    /// Fill the caches a newly loaded module needs: its subscriptions to
    /// function views, and its own function views' existing subscribers.
    pub(crate) fn prime_views(&self, loaded: &str) {
        let modules: Vec<Arc<Module>> = self.modules.lock().values().cloned().collect();
        for owner in &modules {
            for view in &owner.schema.views {
                if !matches!(view.kind, ViewKind::Function { .. }) {
                    continue;
                }
                for sub in self.view_subscribers(&owner.schema, &view.name) {
                    let concerned = owner.schema.name == loaded || matches!(&sub.who, Who::Local(m) if m == loaded);
                    if concerned {
                        self.prime_view(owner, view, &sub.viewer);
                    }
                }
            }
        }
    }

    /// Fill the cache of `view` for `viewer`, without publishing anything.
    pub(crate) fn prime_view(&self, owner: &Arc<Module>, view: &ViewSchema, viewer: &Viewer) {
        let viewer = Self::cache_viewer(view, viewer);
        if let Err(err) = self.rerun_view(owner, view, &viewer) {
            self.logger.log(
                &format!("View '{}.{}' failed: {err}", owner.schema.name, view.name),
                LogSource::Runtime,
                LogLevel::Error,
            );
        }
    }
}

fn event_for(owner: &str, view: &str, kind: Kind, old: Option<Row>, new: Option<Row>) -> EventInstance {
    event(owner, view, kind, old, new)
}
