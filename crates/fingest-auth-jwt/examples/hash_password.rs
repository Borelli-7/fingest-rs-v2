//! Dev utility: print a bcrypt hash for a password.
//!
//! Used to generate the seeded credentials in `migrations/`, and handy when resetting a
//! local account. Never point this at a production password.
//!
//! ```text
//! cargo run -p fingest-auth-jwt --example hash_password -- password123
//! ```

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(password) = args.next() else {
        eprintln!("usage: hash_password <password> [cost]");
        std::process::exit(2);
    };
    let cost: u32 = args
        .next()
        .map_or(Ok(bcrypt::DEFAULT_COST), |c| c.parse())
        .unwrap_or_else(|_| {
            eprintln!("cost must be an integer");
            std::process::exit(2);
        });

    match bcrypt::hash(&password, cost) {
        Ok(hash) => println!("{hash}"),
        Err(err) => {
            eprintln!("hashing failed: {err}");
            std::process::exit(1);
        }
    }
}
