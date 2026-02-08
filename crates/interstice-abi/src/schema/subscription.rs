use serde::{Deserialize, Serialize};

use crate::SubscriptionEventSchema;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubscriptionSchema {
    pub reducer_name: String,
    pub event: SubscriptionEventSchema,
}
