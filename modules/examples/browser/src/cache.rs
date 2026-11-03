//! Instance caches: parsed documents derived from the tables in
//! [`crate::model`]. Each pool instance builds its own on demand; nothing
//! here is state (losing it costs a re-parse, never correctness).

use blitz_html::HtmlDocument;
use blitz_traits::net::NetHandler;
use interstice_sdk::instance_cache;
use std::collections::HashMap;

/// What the chrome document currently displays (compared against the
/// tables each frame; only differences are applied).
#[derive(Clone, PartialEq)]
pub struct ChromeView {
    /// (id, title, loading, selected, favicon)
    pub tabs: Vec<(u64, String, bool, bool, Option<String>)>,
    /// (back, forward, loading)
    pub nav: (bool, bool, bool),
    pub url: String,
    pub editing: bool,
    pub url_text: String,
}

pub struct ChromeCache {
    pub doc: HtmlDocument,
    pub applied: Option<ChromeView>,
    /// Identity of this cache's pixels (0 = not allocated yet), see `Ui`.
    pub stamp: u64,
    pub dirty: bool,
}

impl ChromeCache {
    pub fn new(doc: HtmlDocument) -> Self {
        ChromeCache { doc, applied: None, stamp: 0, dirty: true }
    }
}

pub struct DocCache {
    pub doc: HtmlDocument,
    /// `seq` of the last `DocEvent` this copy has applied.
    pub ev_seq: u64,
    /// Sub-resource fetches waiting for a `Resource` row.
    pub waiting: HashMap<String, Vec<Box<dyn NetHandler>>>,
    /// `Session.res_rev` last checked against `waiting`.
    pub res_rev: u64,
    pub stamp: u64,
    pub dirty: bool,
    /// The page's script realm (documents without scripts have none).
    pub js: Option<crate::js::Js>,
    /// Length of the document's `DocEvent` log as far as this copy knows
    /// (replayed + appended), for the compaction budget.
    pub logged: usize,
    /// Paint commands of the last paint, to find the damaged region of the
    /// next one.
    pub scene: Option<anyrender::Scene>,
}

impl DocCache {
    pub fn new(doc: HtmlDocument) -> Self {
        DocCache { doc, ev_seq: 0, waiting: HashMap::new(), res_rev: u64::MAX, stamp: 0, dirty: true, js: None,
            logged: 0,
            scene: None }
    }
}

#[derive(Default)]
pub struct Caches {
    pub chrome: Option<ChromeCache>,
    pub docs: HashMap<u64, DocCache>,
}

instance_cache! {
    pub static CACHES: Caches;
}
