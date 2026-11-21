mod bertbox;
mod bxwrp;
mod cloneholio;
mod jellium_desktop;
mod moon;
mod noted;
mod uncomment;
mod zoekt_simple;

use crate::tools::bertbox::packages::Package;

pub static ALL: &[Package] = &[
    bertbox::PACKAGE,
    bxwrp::PACKAGE,
    cloneholio::PACKAGE,
    noted::PACKAGE,
    uncomment::PACKAGE,
    moon::PACKAGE,
    zoekt_simple::PACKAGE,
    jellium_desktop::PACKAGE,
];
