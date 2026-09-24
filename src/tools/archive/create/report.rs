use std::path::Path;

const SIZE_UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
const NO_CHANGE: &str = "-------";

pub struct SizeReport {
    before_total: u64,
    after_total: u64,
    expected: usize,
    printed: usize,
}

impl SizeReport {
    pub fn new(expected: usize) -> Self {
        Self {
            before_total: 0,
            after_total: 0,
            expected,
            printed: 0,
        }
    }

    pub fn print(&mut self, before: u64, after: u64, archive: &Path) {
        self.printed += 1;
        self.before_total += before;
        self.after_total += after;
        let change = match saved_percent(before, after) {
            Some(percent) => format!("{percent:>6}%"),
            None => NO_CHANGE.to_owned(),
        };
        let width = self.expected.to_string().len();
        println!(
            "({:>width$}/{})\t{change}\t{}",
            self.printed,
            self.expected,
            archive.display()
        );
    }

    pub fn print_total(&self) {
        if self.printed == 0 {
            return;
        }
        match saved_percent(self.before_total, self.after_total) {
            Some(percent) => {
                let saved = self.before_total as f64 - self.after_total as f64;
                println!("Total: {percent}% ({})", human_size(saved));
            }
            None => println!("Total: -"),
        }
    }
}

fn saved_percent(before: u64, after: u64) -> Option<String> {
    if before == after || before == 0 {
        return None;
    }
    let percent = (before as f64 - after as f64) / before as f64 * 100.0;
    let text = format!("{percent:.2}");
    if text.parse::<f64>() == Ok(0.0) {
        Some(format!("<{text}"))
    } else {
        Some(text)
    }
}

fn human_size(bytes: f64) -> String {
    let mut size = bytes;
    let mut unit = SIZE_UNITS[0];
    for next in SIZE_UNITS.into_iter().skip(1) {
        if size.abs() < 1024.0 {
            break;
        }
        size /= 1024.0;
        unit = next;
    }
    format!("{size:.1}{unit}")
}
