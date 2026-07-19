use keyring::Entry;

fn main() {
    let entry = Entry::new("com.dinero.app", "dinero-base-key").unwrap();
    match entry.get_password() {
        Ok(pw) => println!("Got password: {}", pw),
        Err(e) => {
            println!("No password found: {}. Setting it...", e);
            entry.set_password("test_password").unwrap();
            println!("Password set. Retrieving again...");
            let pw = entry.get_password().unwrap();
            println!("Got password: {}", pw);
        }
    }
}
