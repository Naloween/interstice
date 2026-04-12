//! Reducer table access metadata uses the same scoping as subscriptions:
//! - `table` — current node (must be configured) + current module + `table`
//! - `module.table` — current node + `module` + `table`
//! - `node.module.table` — fully explicit (no local node name required)
//!
//! All comparisons use a normalized `node.module.table` key (lowercase segments).

use crate::runtime::ReplicaBinding;
use std::collections::HashSet;

/// Normalize a user-written access entry from `#[reducer(reads = [...], ...)]` / module schema.
pub fn normalize_user_table_ref(
    entry: &str,
    local_node: Option<&str>,
    current_module: &str,
) -> Result<String, String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return Err("empty table access entry".into());
    }
    let parts: Vec<&str> = entry
        .split('.')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let cm = current_module.trim().to_lowercase();
    match parts.len() {
        1 => {
            let ln = local_node.ok_or_else(|| {
                "local node display name is not configured; use a full `node.module.table` path, or pass the node name when creating the runtime".to_string()
            })?;
            Ok(format!(
                "{}.{}.{}",
                ln.to_lowercase(),
                cm,
                parts[0].to_lowercase()
            ))
        }
        2 => {
            let ln = local_node.ok_or_else(|| {
                "local node display name is not configured; use `node.module.table` when you omit the node segment".to_string()
            })?;
            Ok(format!(
                "{}.{}.{}",
                ln.to_lowercase(),
                parts[0].to_lowercase(),
                parts[1].to_lowercase()
            ))
        }
        3 => Ok(format!(
            "{}.{}.{}",
            parts[0].to_lowercase(),
            parts[1].to_lowercase(),
            parts[2].to_lowercase()
        )),
        _ => Err(format!(
            "invalid table access {:?}: expected `table`, `module.table`, or `node.module.table`",
            entry
        )),
    }
}

pub fn expand_access_set(
    entries: &[String],
    local_node: Option<&str>,
    current_module: &str,
) -> Result<HashSet<String>, String> {
    let mut out = HashSet::new();
    for e in entries {
        out.insert(normalize_user_table_ref(e, local_node, current_module)?);
    }
    Ok(out)
}

/// Map a physical table name (native or `__replica__...`) to the same `node.module.table` key as [`normalize_user_table_ref`].
pub fn canonical_physical_name(
    physical: &str,
    owner_module: &str,
    local_node: Option<&str>,
    replica_bindings: &[ReplicaBinding],
) -> Result<String, String> {
    for b in replica_bindings {
        if b.owner_module_name == owner_module && b.local_table_name == physical {
            return Ok(format!(
                "{}.{}.{}",
                b.source_node_name.to_lowercase(),
                b.source_module_name.to_lowercase(),
                b.source_table_name.to_lowercase()
            ));
        }
    }
    let ln = local_node.ok_or_else(|| {
        format!(
            "local node display name is not configured; cannot resolve ACL for physical table {:?}",
            physical
        )
    })?;
    Ok(format!(
        "{}.{}.{}",
        ln.to_lowercase(),
        owner_module.to_lowercase(),
        physical.to_lowercase()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_part_is_stable_without_local_node() {
        assert_eq!(
            normalize_user_table_ref(
                "hello-example.hello-example.greetings",
                None,
                "caller-example"
            )
            .unwrap(),
            "hello-example.hello-example.greetings"
        );
    }

    #[test]
    fn one_part_needs_local_node() {
        assert!(normalize_user_table_ref("greetings", None, "caller-example").is_err());
        assert_eq!(
            normalize_user_table_ref("greetings", Some("caller-example"), "caller-example")
                .unwrap(),
            "caller-example.caller-example.greetings"
        );
    }

    #[test]
    fn two_part_needs_local_node() {
        assert_eq!(
            normalize_user_table_ref(
                "other-module.tbl",
                Some("n1"),
                "caller-example"
            )
            .unwrap(),
            "n1.other-module.tbl"
        );
    }
}
