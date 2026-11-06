//! The kernel side of security (DESIGN §7.4): the node's config (owners,
//! listen address, file root), who may call what, and where the File
//! authority may go.

use crate::{error::IntersticeError, node::NodeId};
use interstice_abi::{CallVisibility, ModuleSchema, ModuleVisibility, TableVisibility};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

pub const CONFIG_FILE: &str = "node.toml";
pub const DEFAULT_LISTEN: &str = "127.0.0.1";

/// An identity allowed to administer the node. Ids are bound to keys
/// (DESIGN §7.4), so no secret is stored; an old `token` field is ignored.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Owner {
    pub id: String,
}

/// `node.toml` in the node directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Host to listen on: `127.0.0.1` (default) or `0.0.0.0` to expose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<String>,
    /// Where the File authority may go; default `<node>/files`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_root: Option<PathBuf>,
    #[serde(default)]
    pub owners: Vec<Owner>,
}

impl NodeConfig {
    pub fn load(node_dir: &Path) -> Result<Self, IntersticeError> {
        match std::fs::read_to_string(node_dir.join(CONFIG_FILE)) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| IntersticeError::Internal(format!("{CONFIG_FILE}: {e}"))),
            Err(_) => Ok(Self::default()),
        }
    }

    pub fn save(&self, node_dir: &Path) -> Result<(), IntersticeError> {
        let text = toml::to_string_pretty(self).map_err(|e| IntersticeError::Internal(e.to_string()))?;
        std::fs::write(node_dir.join(CONFIG_FILE), text)
            .map_err(|e| IntersticeError::Internal(format!("{CONFIG_FILE}: {e}")))
    }

    pub fn listen_host(&self) -> &str {
        self.listen.as_deref().unwrap_or(DEFAULT_LISTEN)
    }

    pub fn files_root(&self, node_dir: &Path) -> PathBuf {
        self.files_root.clone().unwrap_or_else(|| node_dir.join("files"))
    }

    /// Add an owner; `true` if it was not one yet.
    pub fn add_owner(&mut self, id: &str) -> bool {
        if self.owners.iter().any(|o| o.id == id) {
            return false;
        }
        self.owners.push(Owner { id: id.into() });
        true
    }
}

/// The kernel's security state, shared by the runtime.
pub struct Security {
    pub node_id: NodeId,
    owners: RwLock<HashSet<NodeId>>,
    files_root: RwLock<PathBuf>,
}

/// Who is calling, from what every job carries (DESIGN §7.4).
#[derive(Debug, PartialEq)]
pub enum Caller<'a> {
    Kernel,
    Module(&'a str),
    Owner,
    Peer,
}

impl Security {
    pub fn new(node_id: NodeId, files_root: PathBuf) -> Self {
        Self { node_id, owners: RwLock::new(HashSet::new()), files_root: RwLock::new(files_root) }
    }

    pub fn apply(&self, config: &NodeConfig, node_dir: &Path) {
        *self.owners.write() = config.owners.iter().filter_map(|o| o.id.parse().ok()).collect();
        *self.files_root.write() = config.files_root(node_dir);
    }

    pub fn is_owner(&self, id: &NodeId) -> bool {
        self.owners.read().contains(id)
    }

    pub fn caller<'a>(&self, node_id: NodeId, module: &'a str) -> Caller<'a> {
        if node_id == self.node_id {
            if module.is_empty() { Caller::Kernel } else { Caller::Module(module) }
        } else if self.is_owner(&node_id) {
            Caller::Owner
        } else {
            Caller::Peer
        }
    }

    /// May this caller run reducer (or query) `name` of `schema`?
    pub fn check_call(
        &self,
        schema: &ModuleSchema,
        name: &str,
        query: bool,
        caller_node: NodeId,
        caller_module: &str,
    ) -> Result<(), IntersticeError> {
        let visibility = schema.call_visibility(name, query).unwrap_or(CallVisibility::Internal);
        let allowed = match self.caller(caller_node, caller_module) {
            Caller::Kernel | Caller::Owner => true,
            Caller::Module(m) if m == schema.name => true,
            Caller::Module(_) => matches!(visibility, CallVisibility::Public | CallVisibility::Local),
            Caller::Peer => visibility == CallVisibility::Public,
        };
        if allowed {
            return Ok(());
        }
        let who = match caller_module {
            "" => format!("node {caller_node}"),
            m => format!("module '{m}'"),
        };
        Err(IntersticeError::PermissionDenied(format!(
            "{} '{}.{name}' is {:?}; {who} may not call it",
            if query { "query" } else { "reducer" },
            schema.name,
            visibility
        )))
    }

    /// May a peer read table `table` of `schema` remotely (subscription, sync)?
    pub fn check_remote_table(&self, schema: &ModuleSchema, table: &str, peer: NodeId) -> Result<(), IntersticeError> {
        if self.is_owner(&peer) {
            return Ok(());
        }
        // A view: it must be public, in a public module (DESIGN §7.5).
        if schema.view(table).is_some() {
            return if schema.visibility == ModuleVisibility::Public
                && schema.view_visibility(table) == Some(CallVisibility::Public)
            {
                Ok(())
            } else {
                Err(IntersticeError::PermissionDenied(format!("view '{}.{table}' is not public", schema.name)))
            };
        }
        let table_public = schema
            .tables
            .iter()
            .find(|t| t.name == table)
            .is_some_and(|t| t.visibility == TableVisibility::Public);
        if schema.visibility == ModuleVisibility::Public && table_public {
            Ok(())
        } else {
            Err(IntersticeError::PermissionDenied(format!(
                "table '{}.{table}' is not public",
                schema.name
            )))
        }
    }

    pub fn files_root(&self) -> PathBuf {
        self.files_root.read().clone()
    }

    /// `path` resolved inside the file root, or why not. Relative paths
    /// start at the root; `..` and paths leaving it (symlinks included,
    /// for the part that exists) are refused.
    pub fn confine(&self, path: &str) -> Result<PathBuf, String> {
        let root = self.files_root();
        std::fs::create_dir_all(&root).map_err(|e| format!("file root {}: {e}", root.display()))?;
        let p = Path::new(path);
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(format!("'{path}': `..` is not allowed"));
        }
        let joined = if p.is_absolute() { p.to_path_buf() } else { root.join(p) };
        let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
        // The longest part of the path that exists, resolved.
        let mut existing = joined.as_path();
        while !existing.exists() {
            match existing.parent() {
                Some(parent) => existing = parent,
                None => break,
            }
        }
        let resolved = existing.canonicalize().unwrap_or_else(|_| existing.to_path_buf());
        if !resolved.starts_with(&canonical_root) {
            return Err(format!("'{path}' is outside the node's file root"));
        }
        Ok(joined)
    }

    /// `path` relative to the file root, for file-watch events.
    pub fn relative(&self, path: &Path) -> String {
        let root = self.files_root();
        let canonical = root.canonicalize().unwrap_or(root.clone());
        path.strip_prefix(&canonical)
            .or_else(|_| path.strip_prefix(&root))
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests_support {
    use super::*;
    use interstice_abi::{ReducerSchema, SubscriptionEventSchema, SubscriptionSchema, Version};

    pub fn schema(visibility: ModuleVisibility) -> ModuleSchema {
        let mut internal = ReducerSchema::new("secret", vec![], vec![], vec![], vec![], vec![]);
        internal.visibility = CallVisibility::Internal;
        let mut public_handler = ReducerSchema::new("open_handler", vec![], vec![], vec![], vec![], vec![]);
        public_handler.visibility = CallVisibility::Public;
        ModuleSchema {
            abi_version: interstice_abi::ABI_VERSION,
            name: "m".into(),
            version: Version { major: 0, minor: 1, patch: 0 },
            visibility,
            reducers: vec![
                ReducerSchema::new("plain", vec![], vec![], vec![], vec![], vec![]),
                ReducerSchema::new("on_init", vec![], vec![], vec![], vec![], vec![]),
                internal,
                public_handler,
            ],
            queries: vec![],
            tables: vec![],
            subscriptions: vec![
                SubscriptionSchema { reducer_name: "on_init".into(), event: SubscriptionEventSchema::Init },
                SubscriptionSchema { reducer_name: "open_handler".into(), event: SubscriptionEventSchema::Load },
            ],
            type_definitions: Default::default(),
            authorities: vec![],
            module_dependencies: vec![],
            node_dependencies: vec![],
            replicated_tables: vec![],
            views: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::tests_support::schema;

    #[test]
    fn call_rules() {
        let me = uuid::Uuid::new_v4();
        let owner = uuid::Uuid::new_v4();
        let peer = uuid::Uuid::new_v4();
        let sec = Security::new(me, std::env::temp_dir());
        let mut config = NodeConfig::default();
        config.add_owner(&owner.to_string());
        sec.apply(&config, Path::new("/tmp"));

        let public = schema(ModuleVisibility::Public);
        let ok = |s: &ModuleSchema, f: &str, node, module| sec.check_call(s, f, false, node, module).is_ok();
        // Public module: plain reducers are public.
        assert!(ok(&public, "plain", peer, ""));
        assert!(ok(&public, "plain", me, "other"));
        // Event handlers are internal by default; explicit public wins.
        assert!(!ok(&public, "on_init", peer, ""));
        assert!(!ok(&public, "on_init", me, "other"));
        assert!(ok(&public, "open_handler", peer, ""));
        // Internal: own module, the kernel and owners only.
        assert!(!ok(&public, "secret", me, "other"));
        assert!(ok(&public, "secret", me, "m"));
        assert!(ok(&public, "secret", me, ""));
        assert!(ok(&public, "secret", owner, ""));
        // Private module: plain reducers are local.
        let private = schema(ModuleVisibility::Private);
        assert!(!ok(&private, "plain", peer, ""));
        assert!(ok(&private, "plain", me, "other"));
        assert!(ok(&private, "plain", owner, ""));
        // Unknown functions are treated as internal.
        assert!(!ok(&public, "nope", peer, ""));
    }

    #[test]
    fn file_root_confinement() {
        let root = std::env::temp_dir().join(format!("itc-root-{}", uuid::Uuid::new_v4()));
        let sec = Security::new(uuid::Uuid::new_v4(), root.clone());
        assert_eq!(sec.confine("a/b.txt").unwrap(), root.join("a/b.txt"));
        assert!(sec.confine("../escape").is_err());
        assert!(sec.confine("/etc/passwd").is_err());
        assert!(sec.confine(root.join("x").to_str().unwrap()).is_ok());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", root.join("link")).unwrap();
            assert!(sec.confine("link/passwd").is_err());
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}

#[cfg(test)]
mod remote_schema_tests {
    #[test]
    fn to_remote_keeps_only_public_functions() {
        let schema = super::tests_support::schema(interstice_abi::ModuleVisibility::Public).to_remote();
        let names: Vec<_> = schema.reducers.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["plain", "open_handler"]);
    }
}
