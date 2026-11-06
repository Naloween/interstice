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

/// The ABI this build speaks (DESIGN §10).
pub const ABI_VERSION: u16 = 16;
/// The oldest ABI a host of this build still loads. Versions in
/// `MIN_ABI_VERSION..=ABI_VERSION` only added host imports or optional
/// fields, so modules built for them run unchanged.
pub const MIN_ABI_VERSION: u16 = 15;

/// Whether a module built for `version` loads on this host.
pub fn abi_supported(version: u16) -> bool {
    (MIN_ABI_VERSION..=ABI_VERSION).contains(&version)
}
