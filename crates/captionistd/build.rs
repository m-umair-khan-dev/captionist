// Rebuild when the Sentry DSN changes (it's compiled in via option_env!).
fn main() {
    println!("cargo:rerun-if-env-changed=CAPTIONIST_SENTRY_DSN");
}
