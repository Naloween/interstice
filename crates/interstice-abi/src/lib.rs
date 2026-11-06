mod authority;
mod codec;
mod context;
mod error;
mod host_calls;
mod interstice_type;
mod interstice_type_def;
mod interstice_value;
mod row;
mod schema;

pub use authority::*;
pub use codec::*;
pub use context::*;
pub use error::*;
pub use host_calls::*;
pub use interstice_abi_macros;
pub use interstice_type::*;
pub use interstice_type_def::*;
pub use interstice_value::*;
pub use row::*;
pub use schema::*;

/// The ABI this build speaks (DESIGN §10). A host loads only modules built
/// for it and reads only data written under it.
pub const ABI_VERSION: u16 = 17;
