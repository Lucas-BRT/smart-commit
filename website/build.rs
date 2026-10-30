use topcoat::tailwind::BuildConfig;

fn main() {
    let config = BuildConfig::new().input("styles.css");
    // The Docker build points this at a musl Tailwind CLI; elsewhere Topcoat
    // downloads its pinned glibc one.
    let config = if std::env::var_os("TAILWIND_CLI").is_some() {
        config.executable_env("TAILWIND_CLI")
    } else {
        config
    };
    config.render().unwrap();
}
