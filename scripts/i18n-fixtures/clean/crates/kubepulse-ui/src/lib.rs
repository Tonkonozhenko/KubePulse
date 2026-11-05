fn f() { x.label(t!("a.b", name = "n")); y.child("…"); z.id("some-id"); }
#[cfg(test)]
mod tests { fn g() { x.label("Allowed in tests"); } }
