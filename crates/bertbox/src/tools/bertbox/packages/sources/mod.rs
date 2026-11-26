mod aweber_cli;
mod bertbox;
mod bxwrp;
mod cloneholio;
mod grafeo;
mod jellium_desktop;
mod jevrev;
mod moon;
mod noted;
mod uncomment;
mod zoekt_simple;

use crate::tools::bertbox::packages::Source;

pub static ALL: &[Source] = &[
    bertbox::SOURCE,
    aweber_cli::SOURCE,
    bxwrp::SOURCE,
    cloneholio::SOURCE,
    noted::SOURCE,
    jevrev::SOURCE,
    uncomment::SOURCE,
    moon::SOURCE,
    zoekt_simple::SOURCE,
    grafeo::SOURCE,
    jellium_desktop::SOURCE,
];
