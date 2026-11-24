// Si `assets/pdfium.dll` est présent (fourni par la CI de release), on l'embarque dans l'exécutable
// pour afficher les PDF dans l'app. Sans lui, l'app compile quand même (affichage indisponible).
fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_pdfium)");
    println!("cargo:rerun-if-changed=assets/pdfium.dll");
    if std::path::Path::new("assets/pdfium.dll").exists() {
        println!("cargo:rustc-cfg=has_pdfium");
    }
}
