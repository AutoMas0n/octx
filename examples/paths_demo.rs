use octx::paths;

fn main() {
    println!("📁  Data  dir: {}", paths::data_dir().display());
    println!("📁  Config dir: {}", paths::config_dir().display());
    println!("📁  Bin    dir: {}", paths::bin_dir().display());
    println!("📁  Skills dir: {}", paths::skills_dir().display());
}
