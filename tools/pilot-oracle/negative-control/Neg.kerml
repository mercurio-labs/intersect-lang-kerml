package NegK {
    classifier C;
    feature f : Missing;
    feature g redefines nothere;
    classifier D specializes C, C;
    feature h;
}
