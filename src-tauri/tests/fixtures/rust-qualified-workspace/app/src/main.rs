use core_alias::{Maker, PublicStore as ImportedStore};

fn create_both() {
    ImportedStore::new(); core_alias::store::Store::new();
}

fn create_unknown() {
    Unknown::new();
}

fn create_with_ufcs() {
    <ImportedStore as Maker>::make();
}

fn create_with_turbofish() {
    core_alias::store::Generic::<u8>::new();
}

fn create_with_raw_identifiers() {
    core_alias::store::r#Raw::r#build();
}

mod ambiguity {
    use core_alias::other::Store as Choice;
    use core_alias::store::Store as Choice;

    pub fn create() {
        Choice::new();
    }
}

mod glob_only {
    use core_alias::*;

    pub fn create() {
        PublicStore::new();
    }
}

fn create_missing_dependency() {
    missing_dependency::Store::new();
}

fn create_reexport_cycle() {
    core_alias::cycle_a::CycleStore::new();
}

fn main() {
    create_both();
    create_unknown();
    create_with_ufcs();
    create_with_turbofish();
    create_with_raw_identifiers();
    ambiguity::create();
    glob_only::create();
    create_missing_dependency();
    create_reexport_cycle();
}
