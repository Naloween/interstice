//! Binding TOML (`src/bindings/*.toml`) parsing for the SDK. Lives here (not `interstice-abi`)
//! so the ABI stays plain serde/postcard types; this layer expands string shorthands like `"layer"`
//! into [`interstice_abi::ReducerTableRef`] (using [`ModuleSelection::Current`] for one-segment paths).

use interstice_abi::{
    FieldDef, ModuleSchema, ModuleSelection, NodeSchema, NodeSelection, ReducerSchema,
    ReducerTableRef,
};
use serde::Deserialize;
use serde::de::Error as SerdeDeError;

/// Same rules as `#[reducer]` / proc-macros: `table`, `module.table`, or `node.module.table`.
/// One segment uses [`ModuleSelection::Current`] (this module at runtime), not an embedded package name.
pub fn parse_reducer_table_ref_shorthand(entry: &str) -> Result<ReducerTableRef, String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return Err("empty table access entry".into());
    }
    let parts: Vec<&str> = entry
        .split('.')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    match parts.len() {
        1 => Ok(ReducerTableRef {
            node_selection: NodeSelection::Current,
            module_selection: ModuleSelection::Current,
            table_name: parts[0].to_lowercase(),
        }),
        2 => Ok(ReducerTableRef {
            node_selection: NodeSelection::Current,
            module_selection: ModuleSelection::Other(parts[0].to_lowercase()),
            table_name: parts[1].to_lowercase(),
        }),
        3 => Ok(ReducerTableRef {
            node_selection: NodeSelection::Other(parts[0].to_lowercase()),
            module_selection: ModuleSelection::Other(parts[1].to_lowercase()),
            table_name: parts[2].to_lowercase(),
        }),
        _ => Err(format!(
            "invalid table access {entry:?}: expected `table`, `module.table`, or `node.module.table`"
        )),
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReducerAccessWire {
    /// Must be listed before [`ReducerTableRef`]: strings are not valid as structured refs.
    Short(String),
    Full(ReducerTableRef),
}

#[derive(Deserialize)]
struct ReducerTomlWire {
    name: String,
    arguments: Vec<FieldDef>,
    #[serde(default)]
    reads: Vec<ReducerAccessWire>,
    #[serde(default)]
    inserts: Vec<ReducerAccessWire>,
    #[serde(default)]
    updates: Vec<ReducerAccessWire>,
    #[serde(default)]
    deletes: Vec<ReducerAccessWire>,
}

fn wire_vec_to_refs(v: Vec<ReducerAccessWire>) -> Result<Vec<ReducerTableRef>, toml::de::Error> {
    v.into_iter()
        .map(|w| match w {
            ReducerAccessWire::Full(r) => Ok(r),
            ReducerAccessWire::Short(s) => parse_reducer_table_ref_shorthand(&s)
                .map_err(|e| <toml::de::Error as SerdeDeError>::custom(e)),
        })
        .collect()
}

fn parse_reducer_toml_value(v: toml::Value) -> Result<ReducerSchema, toml::de::Error> {
    let w: ReducerTomlWire = ReducerTomlWire::deserialize(v)?;
    Ok(ReducerSchema {
        name: w.name,
        arguments: w.arguments,
        reads: wire_vec_to_refs(w.reads)?,
        inserts: wire_vec_to_refs(w.inserts)?,
        updates: wire_vec_to_refs(w.updates)?,
        deletes: wire_vec_to_refs(w.deletes)?,
    })
}

fn parse_reducers_toml(reducers_val: Option<toml::Value>) -> Result<Vec<ReducerSchema>, toml::de::Error> {
    let Some(rv) = reducers_val else {
        return Ok(Vec::new());
    };
    let arr = rv
        .as_array()
        .ok_or_else(|| <toml::de::Error as SerdeDeError>::custom(
            "`reducers` must be a TOML array",
        ))?;
    arr.iter()
        .map(|r| parse_reducer_toml_value(r.clone()))
        .collect()
}

/// Parse a flat module binding document (`src/bindings/foo.toml` for the current crate).
pub fn module_schema_from_toml_str(s: &str) -> Result<ModuleSchema, toml::de::Error> {
    let v: toml::Value = toml::from_str(s)?;
    module_schema_from_toml_value(v)
}

/// Parse a flat module document from a [`toml::Value`].
pub fn module_schema_from_toml_value(v: toml::Value) -> Result<ModuleSchema, toml::de::Error> {
    let mut table = match v {
        toml::Value::Table(t) => t,
        _ => {
            return Err(<toml::de::Error as SerdeDeError>::custom(
                "module schema TOML must be a table at the root",
            ))
        }
    };

    let reducers_raw = table.remove("reducers");
    table.insert("reducers".into(), toml::Value::Array(Vec::new()));

    let body = toml::Value::Table(table);
    let mut schema: ModuleSchema = toml::from_str(&toml::to_string(&body).map_err(|e| {
        <toml::de::Error as SerdeDeError>::custom(format!("TOML re-serialize: {e}"))
    })?)?;
    schema.reducers = parse_reducers_toml(reducers_raw)?;
    Ok(schema)
}

/// Parse a node binding document (`name` + `address` + `[[modules]]`).
pub fn node_schema_from_toml_str(s: &str) -> Result<NodeSchema, toml::de::Error> {
    let v: toml::Value = toml::from_str(s)?;
    let name = v
        .get("name")
        .and_then(|x| x.as_str())
        .ok_or_else(|| <toml::de::Error as SerdeDeError>::custom("missing `name` in node schema"))?
        .to_string();
    let address = v
        .get("address")
        .and_then(|x| x.as_str())
        .ok_or_else(|| {
            <toml::de::Error as SerdeDeError>::custom("missing `address` in node schema")
        })?
        .to_string();
    let modules_arr = v
        .get("modules")
        .and_then(|x| x.as_array())
        .ok_or_else(|| {
            <toml::de::Error as SerdeDeError>::custom("missing `modules` array in node schema")
        })?;
    let mut modules = Vec::new();
    for m in modules_arr {
        modules.push(module_schema_from_toml_value(m.clone())?);
    }
    Ok(NodeSchema {
        name,
        address,
        modules,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_reducer_table_ref_shorthand;
    use interstice_abi::{ModuleSelection, NodeSelection};

    #[test]
    fn shorthand_parsing_matches_segment_rules() {
        let one = parse_reducer_table_ref_shorthand("greetings").unwrap();
        assert!(matches!(one.node_selection, NodeSelection::Current));
        assert!(matches!(one.module_selection, ModuleSelection::Current));
        assert_eq!(one.table_name, "greetings");

        let three = parse_reducer_table_ref_shorthand("hello-example.hello-example.greetings").unwrap();
        assert!(matches!(
            three.node_selection,
            NodeSelection::Other(ref n) if n == "hello-example"
        ));
        assert!(matches!(
            three.module_selection,
            ModuleSelection::Other(ref m) if m == "hello-example"
        ));
        assert_eq!(three.table_name, "greetings");
    }
}
