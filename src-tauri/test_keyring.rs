fn main() {
    let entry1 = keyring::Entry::new("com.dinero.test", "user1").unwrap();
    entry1.set_password("pass1").unwrap();
    let entry2 = keyring::Entry::new("com.dinero.test", "user2").unwrap();
    entry2.set_password("pass2").unwrap();
    println!("User1: {}", entry1.get_password().unwrap());
    println!("User2: {}", entry2.get_password().unwrap());
    entry1.delete_credential().unwrap();
    entry2.delete_credential().unwrap();
}
