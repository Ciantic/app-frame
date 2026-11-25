use invoice_service::db::schema::SCHEMA;

fn main() {
    match SCHEMA.generate() {
        Ok(sql) => print!("{sql}"),
        Err(problems) => {
            for problem in problems {
                eprintln!("{problem}");
            }
            std::process::exit(1);
        }
    }
}
