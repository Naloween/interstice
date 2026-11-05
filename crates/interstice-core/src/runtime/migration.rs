//! Schema versioning: compare the schema a module's data was written in with
//! the schema of the module being loaded, and plan how to carry the data
//! over (DESIGN §3.3, after SpacetimeDB's automatic migrations).

use std::collections::{HashMap, HashSet};

use interstice_abi::{
    IndexKey, IntersticeType, IntersticeTypeDef, IntersticeValue, ModuleSchema, PersistenceKind,
    Row, TableSchema,
};
use serde::{Deserialize, Serialize};

use crate::error::IntersticeError;

/// How a load treats a plan that needs a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MigrationPolicy {
    /// Apply safe plans, refuse the rest (nothing changes).
    #[default]
    Safe,
    /// Apply the plan as listed, including drops and clears.
    AllowDataLoss,
    /// Discard the module's data and load it as new.
    DeleteData,
}

/// Where a new column's values come from.
#[derive(Debug, Clone)]
pub enum ColumnSource {
    Old(usize),
    Default(IntersticeValue),
}

/// What happens to one persisted table's data.
#[derive(Debug, Clone)]
pub enum TableAction {
    /// Rewrite every row: entry `j` comes from `columns[j]`.
    Map { table: String, columns: Vec<ColumnSource> },
    /// Delete the table's data (cleared, removed, or persistence changed).
    Remove { table: String },
}

#[derive(Debug, Clone)]
pub struct PlanLine {
    pub table: Option<String>,
    pub text: String,
    /// Needs a decision (loses data or breaks the row layout).
    pub asks: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MigrationPlan {
    pub module: String,
    pub lines: Vec<PlanLine>,
    pub actions: Vec<TableAction>,
}

impl MigrationPlan {
    pub fn needs_decision(&self) -> bool {
        self.lines.iter().any(|l| l.asks)
    }

    fn safe(&mut self, table: Option<&str>, text: String) {
        self.lines.push(PlanLine { table: table.map(str::to_string), text, asks: false });
    }

    fn ask(&mut self, table: Option<&str>, text: String) {
        self.lines.push(PlanLine { table: table.map(str::to_string), text, asks: true });
    }

    /// Human-readable plan: `+` safe, `!` needs a decision.
    pub fn render(&self) -> String {
        if self.lines.is_empty() {
            return format!("Module '{}': no schema changes.", self.module);
        }
        let mut out = format!("Migration plan for module '{}':\n", self.module);
        for line in &self.lines {
            let mark = if line.asks { "!" } else { "+" };
            match &line.table {
                Some(t) => out.push_str(&format!("  {mark} {t}: {}\n", line.text)),
                None => out.push_str(&format!("  {mark} {}\n", line.text)),
            }
        }
        if self.needs_decision() {
            out.push_str("Lines marked ! need a decision: abort, migrate (apply as listed), or delete this module's data.\n");
        }
        out
    }
}

/// Plan the migration from `old` to `new`. `rows` reads a table's current
/// rows in its old layout; it is only called for persisted old tables.
pub fn plan(
    old: &ModuleSchema,
    new: &ModuleSchema,
    rows: &mut dyn FnMut(&TableSchema) -> Result<Vec<Row>, IntersticeError>,
) -> Result<MigrationPlan, IntersticeError> {
    let mut plan = MigrationPlan { module: new.name.clone(), ..Default::default() };
    let types = Types { old: &old.type_definitions, new: &new.type_definitions };
    let new_tables: HashMap<&str, &TableSchema> =
        new.tables.iter().map(|t| (t.name.as_str(), t)).collect();
    let old_names: HashSet<&str> = old.tables.iter().map(|t| t.name.as_str()).collect();

    for o in &old.tables {
        let name = o.name.as_str();
        let persisted = o.persistence != PersistenceKind::Ephemeral;
        let Some(n) = new_tables.get(name).copied() else {
            if !persisted {
                plan.safe(Some(name), "table removed (ephemeral)".into());
                continue;
            }
            let count = rows(o)?.len();
            if count == 0 {
                plan.safe(Some(name), "table removed (empty)".into());
            } else {
                plan.ask(Some(name), format!("table removed: its {} drop", rows_text(count)));
            }
            plan.actions.push(TableAction::Remove { table: o.name.clone() });
            continue;
        };
        if !persisted {
            // No persisted data: any change is safe.
            if !same_table(&types, o, n) {
                plan.safe(Some(name), "changed (ephemeral, no data)".into());
            }
            continue;
        }
        let current = rows(o)?;
        let count = current.len();

        // Data that can't follow the table: only clearing is possible.
        let clear_reason = if n.persistence == PersistenceKind::Ephemeral {
            Some("becomes ephemeral".to_string())
        } else if n.persistence != o.persistence {
            Some(format!("persistence changes from {:?} to {:?}", o.persistence, n.persistence))
        } else if o.primary_key.name != n.primary_key.name
            || !types.eq(&o.primary_key.field_type, &n.primary_key.field_type)
        {
            Some(format!(
                "primary key changes from `{}: {}` to `{}: {}`",
                o.primary_key.name, o.primary_key.field_type, n.primary_key.name, n.primary_key.field_type
            ))
        } else {
            None
        };
        if let Some(reason) = clear_reason {
            if count == 0 {
                plan.safe(Some(name), format!("{reason} (empty)"));
            } else {
                plan.ask(Some(name), format!("{reason}: the table is cleared ({} dropped)", rows_text(count)));
            }
            plan.actions.push(TableAction::Remove { table: o.name.clone() });
            continue;
        }

        // Columns, matched by name.
        let mut columns = Vec::new();
        let mut unfillable = Vec::new();
        let mut used = HashSet::new();
        for (j, f) in n.fields.iter().enumerate() {
            let old_pos = o.fields.iter().position(|of| of.name == f.name);
            match old_pos {
                Some(i) if types.eq(&o.fields[i].field_type, &f.field_type) => {
                    used.insert(i);
                    columns.push(ColumnSource::Old(i));
                }
                Some(i) => {
                    let lost = format!(
                        "column `{}` changes type from {} to {}: its values drop",
                        f.name, o.fields[i].field_type, f.field_type
                    );
                    match n.column_default(&f.name) {
                        Some(v) => {
                            plan.ask(Some(name), format!("{lost}, rows get the default"));
                            columns.push(ColumnSource::Default(v));
                        }
                        None => {
                            plan.ask(Some(name), format!("{lost}, and with no default the table is cleared"));
                            unfillable.push(j);
                            columns.push(ColumnSource::Default(IntersticeValue::Void));
                        }
                    }
                    used.insert(i);
                }
                None => match n.column_default(&f.name) {
                    Some(v) => {
                        let text = format!("column `{}` added (default {})", f.name, value_text(&v));
                        // Only an append keeps every old position.
                        if j >= o.fields.len() {
                            plan.safe(Some(name), text);
                        } else {
                            plan.ask(Some(name), format!("{text} in the middle: modules built against the old layout must be rebuilt"));
                        }
                        columns.push(ColumnSource::Default(v));
                    }
                    None => {
                        plan.ask(Some(name), format!(
                            "column `{}` added without a default (`#[default(..)]` or `Option`): the table is cleared",
                            f.name
                        ));
                        unfillable.push(j);
                        columns.push(ColumnSource::Default(IntersticeValue::Void));
                    }
                },
            }
        }
        for (i, f) in o.fields.iter().enumerate() {
            if !used.contains(&i) {
                plan.ask(Some(name), format!("column `{}` removed: its values drop", f.name));
            }
        }
        let kept: Vec<usize> = columns
            .iter()
            .filter_map(|c| if let ColumnSource::Old(i) = c { Some(*i) } else { None })
            .collect();
        if kept.windows(2).any(|w| w[0] > w[1]) {
            plan.ask(Some(name), "columns reordered: modules built against the old layout must be rebuilt".into());
        }

        // Constraints the current rows must satisfy.
        let mut violated = false;
        for idx in &n.indexes {
            let was = o.indexes.iter().find(|i| i.field_name == idx.field_name);
            if was.is_none() {
                plan.safe(Some(name), format!("index on `{}` added", idx.field_name));
            }
            if idx.unique && !was.is_some_and(|w| w.unique) {
                let Some(j) = n.fields.iter().position(|f| f.name == idx.field_name) else { continue };
                if column_has_duplicates(&current, &columns[j]) {
                    plan.ask(Some(name), format!(
                        "`{}` becomes unique but current rows repeat values: the table is cleared",
                        idx.field_name
                    ));
                    violated = true;
                } else if was.is_some() {
                    plan.safe(Some(name), format!("`{}` becomes unique", idx.field_name));
                }
            }
        }
        for idx in &o.indexes {
            if !n.indexes.iter().any(|i| i.field_name == idx.field_name) {
                plan.safe(Some(name), format!("index on `{}` removed", idx.field_name));
            }
        }
        if o.visibility != n.visibility {
            plan.safe(Some(name), format!("visibility changes to {:?}", n.visibility));
        }
        if o.history != n.history || o.retention != n.retention {
            plan.safe(Some(name), "history/retention changes".into());
        }
        if o.primary_key_auto_inc != n.primary_key_auto_inc {
            plan.safe(Some(name), "primary key auto_inc toggled".into());
        }

        let identity = columns.len() == o.fields.len()
            && columns.iter().enumerate().all(|(j, c)| matches!(c, ColumnSource::Old(i) if *i == j));
        if !unfillable.is_empty() || violated {
            if count > 0 {
                plan.ask(Some(name), format!("{} drop", rows_text(count)));
            }
            plan.actions.push(TableAction::Remove { table: o.name.clone() });
        } else if !identity {
            plan.actions.push(TableAction::Map { table: o.name.clone(), columns });
        }
    }

    for n in &new.tables {
        if !old_names.contains(n.name.as_str()) {
            plan.safe(Some(&n.name), "table added".into());
        }
    }

    // Functions: always safe, listed so callers know.
    let names = |v: &mut dyn Iterator<Item = &String>| v.cloned().collect::<HashSet<String>>();
    let (or, nr) = (
        names(&mut old.reducers.iter().map(|r| &r.name)),
        names(&mut new.reducers.iter().map(|r| &r.name)),
    );
    let (oq, nq) = (
        names(&mut old.queries.iter().map(|q| &q.name)),
        names(&mut new.queries.iter().map(|q| &q.name)),
    );
    for r in sorted(nr.difference(&or)) {
        plan.safe(None, format!("reducer `{r}` added"));
    }
    for r in sorted(or.difference(&nr)) {
        plan.safe(None, format!("reducer `{r}` removed: callers may break"));
    }
    for q in sorted(nq.difference(&oq)) {
        plan.safe(None, format!("query `{q}` added"));
    }
    for q in sorted(oq.difference(&nq)) {
        plan.safe(None, format!("query `{q}` removed: callers may break"));
    }
    Ok(plan)
}

/// Plan for data whose schema is unknown (written before `schema.bin`):
/// safe if every row already fits the new schema, otherwise those tables
/// must be cleared.
pub fn plan_unknown(
    new: &ModuleSchema,
    rows: &mut dyn FnMut(&TableSchema) -> Result<Vec<Row>, IntersticeError>,
) -> Result<MigrationPlan, IntersticeError> {
    let mut plan = MigrationPlan { module: new.name.clone(), ..Default::default() };
    for t in &new.tables {
        if t.persistence == PersistenceKind::Ephemeral {
            continue;
        }
        // Rows that don't decode into the new layout surface as errors.
        let current = match rows(t) {
            Ok(r) => r,
            Err(e) => {
                plan.ask(Some(&t.name), format!("stored data can't be read with the new schema ({e}): the table is cleared"));
                plan.actions.push(TableAction::Remove { table: t.name.clone() });
                continue;
            }
        };
        let bad = current.iter().filter(|r| t.validate_row(r, &new.type_definitions).is_err()).count();
        if bad > 0 {
            plan.ask(Some(&t.name), format!(
                "the previous schema is unknown and {} of {} don't fit the new one: the table is cleared",
                bad,
                rows_text(current.len())
            ));
            plan.actions.push(TableAction::Remove { table: t.name.clone() });
        }
    }
    Ok(plan)
}

/// Rewrite one row with a `Map` action.
pub fn map_row(row: Row, columns: &[ColumnSource]) -> Row {
    let entries = columns
        .iter()
        .map(|c| match c {
            ColumnSource::Old(i) => row.entries.get(*i).cloned().unwrap_or(IntersticeValue::Void),
            ColumnSource::Default(v) => v.clone(),
        })
        .collect();
    Row { primary_key: row.primary_key, entries }
}

fn column_has_duplicates(rows: &[Row], source: &ColumnSource) -> bool {
    let ColumnSource::Old(i) = source else {
        // Every row gets the same default.
        return rows.len() > 1;
    };
    let mut seen = HashSet::new();
    for row in rows {
        let Some(v) = row.entries.get(*i) else { continue };
        if let Ok(key) = IndexKey::try_from(v) {
            if !seen.insert(key) {
                return true;
            }
        }
    }
    false
}

fn same_table(types: &Types, o: &TableSchema, n: &TableSchema) -> bool {
    o.fields.len() == n.fields.len()
        && o.fields.iter().zip(&n.fields).all(|(a, b)| a.name == b.name && types.eq(&a.field_type, &b.field_type))
        && o.primary_key.name == n.primary_key.name
        && types.eq(&o.primary_key.field_type, &n.primary_key.field_type)
}

fn rows_text(n: usize) -> String {
    if n == 1 { "1 row".into() } else { format!("{n} rows") }
}

fn value_text(v: &IntersticeValue) -> String {
    match v {
        IntersticeValue::Option(None) => "None".into(),
        IntersticeValue::String(s) => format!("{s:?}"),
        IntersticeValue::Bool(b) => b.to_string(),
        IntersticeValue::U8(x) => x.to_string(),
        IntersticeValue::U32(x) => x.to_string(),
        IntersticeValue::U64(x) => x.to_string(),
        IntersticeValue::I32(x) => x.to_string(),
        IntersticeValue::I64(x) => x.to_string(),
        IntersticeValue::F32(x) => x.to_string(),
        IntersticeValue::F64(x) => x.to_string(),
        other => format!("{other:?}"),
    }
}

fn sorted<'a>(it: impl Iterator<Item = &'a String>) -> Vec<&'a String> {
    let mut v: Vec<_> = it.collect();
    v.sort();
    v
}

/// Structural type equality across the old and new type definitions.
struct Types<'a> {
    old: &'a HashMap<String, IntersticeTypeDef>,
    new: &'a HashMap<String, IntersticeTypeDef>,
}

impl Types<'_> {
    fn eq(&self, a: &IntersticeType, b: &IntersticeType) -> bool {
        self.eq_depth(a, b, 0)
    }

    fn eq_depth(&self, a: &IntersticeType, b: &IntersticeType, depth: u32) -> bool {
        use IntersticeType as T;
        if depth > 32 {
            return true; // recursive types: assume equal past this depth
        }
        match (a, b) {
            (T::Vec(x), T::Vec(y)) | (T::Option(x), T::Option(y)) => self.eq_depth(x, y, depth + 1),
            (T::Tuple(xs), T::Tuple(ys)) => {
                xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| self.eq_depth(x, y, depth + 1))
            }
            (T::Named(x), T::Named(y)) => {
                if x != y {
                    return false;
                }
                match (self.old.get(x), self.new.get(y)) {
                    (Some(dx), Some(dy)) => self.def_eq(dx, dy, depth + 1),
                    (None, None) => true,
                    _ => false,
                }
            }
            _ => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }

    fn def_eq(&self, a: &IntersticeTypeDef, b: &IntersticeTypeDef, depth: u32) -> bool {
        let fields_eq = |xs: &Vec<interstice_abi::FieldDef>, ys: &Vec<interstice_abi::FieldDef>| {
            xs.len() == ys.len()
                && xs.iter().zip(ys).all(|(x, y)| x.name == y.name && self.eq_depth(&x.field_type, &y.field_type, depth))
        };
        match (a, b) {
            (IntersticeTypeDef::Struct { fields: x, .. }, IntersticeTypeDef::Struct { fields: y, .. }) => fields_eq(x, y),
            (IntersticeTypeDef::Enum { variants: x, .. }, IntersticeTypeDef::Enum { variants: y, .. }) => fields_eq(x, y),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interstice_abi::{ColumnDefault, FieldDef, IndexSchema, IndexType, IntersticeType as T, TableVisibility};

    fn table(fields: &[(&str, T)], persistence: PersistenceKind) -> TableSchema {
        TableSchema {
            name: "note".into(),
            type_name: "Note".into(),
            visibility: TableVisibility::Public,
            fields: fields.iter().map(|(n, t)| FieldDef { name: (*n).into(), field_type: t.clone() }).collect(),
            primary_key: FieldDef { name: "id".into(), field_type: T::U64 },
            primary_key_auto_inc: false,
            indexes: Vec::new(),
            persistence,
            retention: None,
            history: None,
            defaults: Vec::new(),
        }
    }

    fn module(tables: Vec<TableSchema>) -> ModuleSchema {
        let mut m = ModuleSchema::empty();
        m.name = "notes".into();
        m.tables = tables;
        m
    }

    fn row(id: u64, entries: Vec<IntersticeValue>) -> Row {
        Row { primary_key: IntersticeValue::U64(id), entries }
    }

    fn with_rows(rows: Vec<Row>) -> impl FnMut(&TableSchema) -> Result<Vec<Row>, IntersticeError> {
        move |_| Ok(rows.clone())
    }

    const L: PersistenceKind = PersistenceKind::Logged;

    #[test]
    fn unchanged_is_empty() {
        let m = module(vec![table(&[("text", T::String)], L)]);
        let p = plan(&m, &m, &mut with_rows(vec![])).unwrap();
        assert!(p.lines.is_empty() && p.actions.is_empty());
    }

    #[test]
    fn append_with_default_is_safe_and_maps() {
        let old = module(vec![table(&[("text", T::String)], L)]);
        let mut t = table(&[("text", T::String), ("pinned", T::Bool)], L);
        t.defaults.push(ColumnDefault::new("pinned", &IntersticeValue::Bool(false)));
        let new = module(vec![t]);
        let p = plan(&old, &new, &mut with_rows(vec![row(1, vec![IntersticeValue::String("a".into())])])).unwrap();
        assert!(!p.needs_decision(), "{}", p.render());
        let TableAction::Map { columns, .. } = &p.actions[0] else { panic!() };
        let mapped = map_row(row(1, vec![IntersticeValue::String("a".into())]), columns);
        assert_eq!(mapped.entries, vec![IntersticeValue::String("a".into()), IntersticeValue::Bool(false)]);
    }

    #[test]
    fn option_column_defaults_to_none() {
        let old = module(vec![table(&[("text", T::String)], L)]);
        let new = module(vec![table(&[("text", T::String), ("tag", T::Option(Box::new(T::String)))], L)]);
        let p = plan(&old, &new, &mut with_rows(vec![])).unwrap();
        assert!(!p.needs_decision(), "{}", p.render());
    }

    #[test]
    fn removal_and_no_default_ask() {
        let old = module(vec![table(&[("text", T::String), ("body", T::String)], L)]);
        let new = module(vec![table(&[("text", T::String)], L)]);
        let p = plan(&old, &new, &mut with_rows(vec![])).unwrap();
        assert!(p.needs_decision());
        let TableAction::Map { columns, .. } = &p.actions[0] else { panic!() };
        assert!(matches!(columns[..], [ColumnSource::Old(0)]));

        let new = module(vec![table(&[("text", T::String), ("n", T::U64)], L)]);
        let p = plan(&old, &new, &mut with_rows(vec![row(1, vec![])])).unwrap();
        assert!(p.needs_decision());
        assert!(matches!(p.actions[0], TableAction::Remove { .. }));
    }

    #[test]
    fn reorder_asks_and_type_change_asks() {
        let old = module(vec![table(&[("a", T::String), ("b", T::U64)], L)]);
        let new = module(vec![table(&[("b", T::U64), ("a", T::String)], L)]);
        assert!(plan(&old, &new, &mut with_rows(vec![])).unwrap().needs_decision());
        let new = module(vec![table(&[("a", T::String), ("b", T::String)], L)]);
        assert!(plan(&old, &new, &mut with_rows(vec![])).unwrap().needs_decision());
    }

    #[test]
    fn table_removal_safe_only_when_empty() {
        let old = module(vec![table(&[("a", T::String)], L)]);
        let new = module(vec![]);
        assert!(!plan(&old, &new, &mut with_rows(vec![])).unwrap().needs_decision());
        let p = plan(&old, &new, &mut with_rows(vec![row(1, vec![IntersticeValue::String("x".into())])])).unwrap();
        assert!(p.needs_decision());
    }

    #[test]
    fn ephemeral_changes_are_safe() {
        let old = module(vec![table(&[("a", T::String)], PersistenceKind::Ephemeral)]);
        let new = module(vec![table(&[("b", T::U64)], PersistenceKind::Ephemeral)]);
        let p = plan(&old, &new, &mut |_| panic!("ephemeral rows are never read")).unwrap();
        assert!(!p.needs_decision() && p.actions.is_empty());
    }

    #[test]
    fn unique_checks_current_rows() {
        let old = module(vec![table(&[("a", T::String)], L)]);
        let mut t = table(&[("a", T::String)], L);
        t.indexes.push(IndexSchema { field_name: "a".into(), index_type: IndexType::Hash, unique: true, auto_inc: false });
        let new = module(vec![t]);
        let s = |v: &str| IntersticeValue::String(v.into());
        assert!(!plan(&old, &new, &mut with_rows(vec![row(1, vec![s("x")]), row(2, vec![s("y")])])).unwrap().needs_decision());
        assert!(plan(&old, &new, &mut with_rows(vec![row(1, vec![s("x")]), row(2, vec![s("x")])])).unwrap().needs_decision());
    }
}
