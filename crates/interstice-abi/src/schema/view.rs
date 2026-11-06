//! Views (DESIGN §7.5): read-only relations a module publishes, defined from
//! its tables and possibly by who reads them.

use crate::{CallVisibility, FieldDef, IntersticeValue, ReducerTableRef, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ViewSchema {
    pub name: String,
    #[serde(default)]
    pub visibility: CallVisibility,
    /// Type definition of the rows on the owner's side (the table's struct
    /// for a filter view, the returned struct for a function view).
    pub row_type: String,
    /// Field of `row_type` that is the primary key.
    pub primary_key: String,
    /// Whether rows depend on the viewer (else: the same for everyone).
    pub per_viewer: bool,
    pub kind: ViewKind,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ViewKind {
    /// Rows of `table` for which `predicate` holds.
    Filter { table: String, predicate: ViewExpr },
    /// Rows returned by the internal query `query`, which reads `reads`.
    Function { query: String, reads: Vec<ReducerTableRef> },
}

/// Who reads a view (DESIGN §7.5): supplied by the kernel. `module` is
/// empty for a reader on another node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Viewer {
    pub node: String,
    pub module: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum ViewerField {
    Node,
    Module,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A literal of a filter condition (kept TOML-friendly).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum ViewLiteral {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

/// A filter view's condition, evaluated by the kernel.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum ViewExpr {
    Column(String),
    Viewer(ViewerField),
    Literal(ViewLiteral),
    Compare { op: CompareOp, left: Box<ViewExpr>, right: Box<ViewExpr> },
    And(Box<ViewExpr>, Box<ViewExpr>),
    Or(Box<ViewExpr>, Box<ViewExpr>),
    Not(Box<ViewExpr>),
}

/// A value during evaluation: numbers compare across integer widths.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
enum Scalar {
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Other,
}

fn scalar(value: &IntersticeValue) -> Scalar {
    use IntersticeValue as V;
    match value {
        V::Bool(b) => Scalar::Bool(*b),
        V::U8(n) => Scalar::Int(*n as i128),
        V::U32(n) => Scalar::Int(*n as i128),
        V::U64(n) => Scalar::Int(*n as i128),
        V::I32(n) => Scalar::Int(*n as i128),
        V::I64(n) => Scalar::Int(*n as i128),
        V::F32(f) => Scalar::Float(*f as f64),
        V::F64(f) => Scalar::Float(*f),
        V::String(s) => Scalar::Str(s.clone()),
        _ => Scalar::Other,
    }
}

impl ViewExpr {
    /// Whether the condition holds for `row` (laid out as `columns`: the
    /// primary key name first, then the other fields in order).
    pub fn holds(&self, row: &Row, primary_key: &str, fields: &[FieldDef], viewer: &Viewer) -> bool {
        matches!(self.eval(row, primary_key, fields, viewer), Scalar::Bool(true))
    }

    fn eval(&self, row: &Row, pk: &str, fields: &[FieldDef], viewer: &Viewer) -> Scalar {
        match self {
            ViewExpr::Column(name) if name == pk => scalar(&row.primary_key),
            ViewExpr::Column(name) => fields
                .iter()
                .position(|f| &f.name == name)
                .and_then(|i| row.entries.get(i))
                .map(scalar)
                .unwrap_or(Scalar::Other),
            ViewExpr::Viewer(ViewerField::Node) => Scalar::Str(viewer.node.clone()),
            ViewExpr::Viewer(ViewerField::Module) => Scalar::Str(viewer.module.clone()),
            ViewExpr::Literal(ViewLiteral::Bool(b)) => Scalar::Bool(*b),
            ViewExpr::Literal(ViewLiteral::Int(n)) => Scalar::Int(*n as i128),
            ViewExpr::Literal(ViewLiteral::Float(f)) => Scalar::Float(*f),
            ViewExpr::Literal(ViewLiteral::Str(s)) => Scalar::Str(s.clone()),
            ViewExpr::Compare { op, left, right } => {
                let (l, r) = (left.eval(row, pk, fields, viewer), right.eval(row, pk, fields, viewer));
                let (l, r) = match (l, r) {
                    (Scalar::Int(a), Scalar::Float(b)) => (Scalar::Float(a as f64), Scalar::Float(b)),
                    (Scalar::Float(a), Scalar::Int(b)) => (Scalar::Float(a), Scalar::Float(b as f64)),
                    pair => pair,
                };
                if std::mem::discriminant(&l) != std::mem::discriminant(&r) || l == Scalar::Other {
                    return Scalar::Bool(false);
                }
                Scalar::Bool(match op {
                    CompareOp::Eq => l == r,
                    CompareOp::Ne => l != r,
                    CompareOp::Lt => l < r,
                    CompareOp::Le => l <= r,
                    CompareOp::Gt => l > r,
                    CompareOp::Ge => l >= r,
                })
            }
            ViewExpr::And(a, b) => Scalar::Bool(
                a.eval(row, pk, fields, viewer) == Scalar::Bool(true)
                    && b.eval(row, pk, fields, viewer) == Scalar::Bool(true),
            ),
            ViewExpr::Or(a, b) => Scalar::Bool(
                a.eval(row, pk, fields, viewer) == Scalar::Bool(true)
                    || b.eval(row, pk, fields, viewer) == Scalar::Bool(true),
            ),
            ViewExpr::Not(a) => match a.eval(row, pk, fields, viewer) {
                Scalar::Bool(b) => Scalar::Bool(!b),
                _ => Scalar::Bool(false),
            },
        }
    }

    /// Whether the condition mentions the viewer.
    pub fn uses_viewer(&self) -> bool {
        match self {
            ViewExpr::Viewer(_) => true,
            ViewExpr::Column(_) | ViewExpr::Literal(_) => false,
            ViewExpr::Compare { left, right, .. } => left.uses_viewer() || right.uses_viewer(),
            ViewExpr::And(a, b) | ViewExpr::Or(a, b) => a.uses_viewer() || b.uses_viewer(),
            ViewExpr::Not(a) => a.uses_viewer(),
        }
    }
}

/// A function view's returned struct as a row: `primary_key` first, then
/// the other fields in declaration order (the layout of a table row).
pub fn struct_to_row(value: IntersticeValue, primary_key: &str) -> Option<Row> {
    let IntersticeValue::Struct { fields, .. } = value else { return None };
    let mut key = None;
    let mut entries = Vec::with_capacity(fields.len().saturating_sub(1));
    for field in fields {
        if field.name == primary_key && key.is_none() {
            key = Some(field.value);
        } else {
            entries.push(field.value);
        }
    }
    Some(Row { primary_key: key?, entries })
}

/// The bindings' row type name of a view: `my_responses` → `MyResponses`.
pub fn view_type_name(view: &str) -> String {
    view.split(['_', '-'])
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IntersticeType;

    fn fields() -> Vec<FieldDef> {
        vec![
            FieldDef { name: "owner".into(), field_type: IntersticeType::String },
            FieldDef { name: "score".into(), field_type: IntersticeType::U32 },
        ]
    }

    fn cmp(op: CompareOp, l: ViewExpr, r: ViewExpr) -> ViewExpr {
        ViewExpr::Compare { op, left: Box::new(l), right: Box::new(r) }
    }

    #[test]
    fn evaluates_conditions() {
        let row = Row { primary_key: IntersticeValue::U64(7), entries: vec!["app".to_string().into(), IntersticeValue::U32(10)] };
        let me = Viewer { node: "n".into(), module: "app".into() };
        let other = Viewer { node: "n".into(), module: "other".into() };
        let mine = cmp(CompareOp::Eq, ViewExpr::Column("owner".into()), ViewExpr::Viewer(ViewerField::Module));
        assert!(mine.holds(&row, "id", &fields(), &me));
        assert!(!mine.holds(&row, "id", &fields(), &other));
        assert!(mine.uses_viewer());
        // Integers compare across widths; keys are columns too.
        let big = cmp(CompareOp::Ge, ViewExpr::Column("score".into()), ViewExpr::Literal(ViewLiteral::Int(10)));
        let key = cmp(CompareOp::Lt, ViewExpr::Column("id".into()), ViewExpr::Literal(ViewLiteral::Int(5)));
        assert!(big.holds(&row, "id", &fields(), &other));
        assert!(!key.holds(&row, "id", &fields(), &other));
        let both = ViewExpr::And(Box::new(mine.clone()), Box::new(ViewExpr::Not(Box::new(key))));
        assert!(both.holds(&row, "id", &fields(), &me));
        // Mismatched types are false, not an error.
        let odd = cmp(CompareOp::Eq, ViewExpr::Column("owner".into()), ViewExpr::Literal(ViewLiteral::Int(1)));
        assert!(!odd.holds(&row, "id", &fields(), &me));
        assert_eq!(view_type_name("my_responses"), "MyResponses");
    }
}
