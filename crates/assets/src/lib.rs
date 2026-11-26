//! Assets (FR-AST, ADR-0034): the IT estate. Analysts register Assets with their
//! identifiers, Asset Type, Asset Status, Location, Supplier, Cost Centre, purchase,
//! warranty and lease dates; Check Out / Check In an Asset to a Holder; Retire it; link
//! it to tickets and contracts; and read its full history. Asset Types, Asset Statuses
//! and Locations are configurable reference data.

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod ports;
pub mod custom_fields;
pub mod extras;
pub mod extras_api;
pub mod extras_ports;
pub mod extras_store;
pub mod import;
pub mod inventory;
