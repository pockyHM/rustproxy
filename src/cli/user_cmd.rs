use anyhow::{Context, Result};

use crate::auth::password;
use crate::db::Database;

pub fn run_add(db_path: &str, username: &str) -> Result<()> {
    let db = Database::open(db_path)?;

    if db.get_user_password_hash(username)?.is_some() {
        anyhow::bail!("user '{username}' already exists");
    }

    let password = read_password("Password: ")?;
    let hash = password::hash_password(&password)?;
    db.create_user(username, &hash)?;
    println!("User '{username}' created.");
    Ok(())
}

pub fn run_list(db_path: &str) -> Result<()> {
    let db = Database::open(db_path)?;
    let users = db.list_users()?;

    if users.is_empty() {
        println!("No admin users.");
        return Ok(());
    }

    println!("{:<5} {:<20} {}", "ID", "Username", "Created");
    for (id, username, created) in &users {
        println!("{id:<5} {username:<20} {created}");
    }
    Ok(())
}

pub fn run_passwd(db_path: &str, username: &str) -> Result<()> {
    let db = Database::open(db_path)?;

    if db.get_user_password_hash(username)?.is_none() {
        anyhow::bail!("user '{username}' not found");
    }

    let password = read_password("New password: ")?;
    let hash = password::hash_password(&password)?;
    db.update_user_password(username, &hash)?;
    println!("Password updated for '{username}'.");
    Ok(())
}

fn read_password(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    let password = rpassword::read_password().context("failed to read password from terminal")?;
    if password.is_empty() {
        anyhow::bail!("password cannot be empty");
    }
    Ok(password)
}
