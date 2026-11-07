mod builder;
mod mapping_binding;
mod mapping_control_record;
mod mapping_entry;
mod mappings;
mod space;

pub use builder::SpaceBuilder;
pub(crate) use mapping_binding::MappingBinding;
pub(crate) use mapping_control_record::MappingControlRecord;
pub(crate) use mapping_entry::MappingEntry;
pub(crate) use mappings::MappingRegistry;
pub use space::Space;

pub(crate) mod wait;
