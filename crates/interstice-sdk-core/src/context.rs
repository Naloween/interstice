use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

pub use interstice_abi::{RawQueryContext, RawReducerContext};


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerContextCurrentModuleTables<Caps = ()> {
    #[serde(skip)]
    _caps: PhantomData<Caps>,
}

impl<Caps> Default for ReducerContextCurrentModuleTables<Caps> {
    fn default() -> Self {
        Self { _caps: PhantomData }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerContextCurrentModule<Caps = ()> {
    pub tables: ReducerContextCurrentModuleTables<Caps>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryContextCurrentModuleTables<Caps = ()> {
    #[serde(skip)]
    _caps: PhantomData<Caps>,
}

impl<Caps> Default for QueryContextCurrentModuleTables<Caps> {
    fn default() -> Self {
        Self { _caps: PhantomData }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryContextCurrentModule<Caps = ()> {
    pub tables: QueryContextCurrentModuleTables<Caps>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerContext<Caps = ()> {
    pub caller_node_id: String,
    pub current: ReducerContextCurrentModule<Caps>,
}

impl<Caps> From<RawReducerContext> for ReducerContext<Caps> {
    fn from(raw: RawReducerContext) -> Self {
        Self {
            caller_node_id: raw.caller_node_id,
            current: ReducerContextCurrentModule {
                tables: ReducerContextCurrentModuleTables::default(),
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryContext<Caps = ()> {
    pub caller_node_id: String,
    pub current: QueryContextCurrentModule<Caps>,
}

impl<Caps> From<RawQueryContext> for QueryContext<Caps> {
    fn from(raw: RawQueryContext) -> Self {
        Self {
            caller_node_id: raw.caller_node_id,
            current: QueryContextCurrentModule {
                tables: QueryContextCurrentModuleTables::default(),
            },
        }
    }
}
