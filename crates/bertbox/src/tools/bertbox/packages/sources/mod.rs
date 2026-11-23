mod bertbox;
mod bxwrp;
mod cloneholio;
mod jellium_desktop;
mod moon;
mod noted;
mod uncomment;
mod zoekt_simple;

use crate::tools::bertbox::packages::Source;

pub static ALL: &[Source] = &[
    bertbox::SOURCE,
    bxwrp::SOURCE,
    cloneholio::SOURCE,
    noted::SOURCE,
    uncomment::SOURCE,
    moon::SOURCE,
    zoekt_simple::SOURCE,
    jellium_desktop::SOURCE,
];
