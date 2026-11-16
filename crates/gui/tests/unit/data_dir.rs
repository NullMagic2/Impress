use super::*;

#[test]
fn a_system_install_uses_the_user_data_folder() {
    // A folder that cannot be written to, as /usr/lib/impress is for users.
    let missing = Path::new("/nonexistent-impress-install/lib");
    assert_eq!(choose(missing, || Some(PathBuf::from("/home/u/.local/share/impress"))), PathBuf::from("/home/u/.local/share/impress"));
    assert_eq!(choose(missing, || None), missing, "without a data folder the old place is kept");
}
