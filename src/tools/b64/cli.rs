use clap::Args as ClapArgs;

#[derive(ClapArgs)]
#[command(about = "Decode nested base64 from arguments, stdin, or the clipboard")]
pub struct B64d {
    #[arg(value_name = "TEXT")]
    pub text: Vec<String>,
}

#[derive(ClapArgs)]
#[command(about = "Encode an argument, stdin, or the clipboard as base64")]
pub struct B64e {
    #[arg(value_name = "TEXT")]
    pub text: Option<String>,
}
