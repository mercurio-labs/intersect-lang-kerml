package Neg {
    part def A { attribute m; }
    part a : Nope;
    part b : A { attribute :>> zz; }
    abstract part def B;
    part e : A { attribute :>> m = 1; attribute :>> m = 2; }
    part f : A [2..1];
}
