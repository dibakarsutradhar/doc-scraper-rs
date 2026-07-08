// Placeholder: implemented in Task 9 (pipeline glue).
// Until then `cargo run` prints this notice.

mod error;
mod fetch;
mod http;
mod markdown;
mod page;
mod sitemap;
mod writer;

fn main() {
    println!("doc-scraper-rs v0.1 — pipeline implemented in Task 9");
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_runs() {
        assert_eq!(2 + 2, 4);
    }
}