// Handwritten milestone-only input for PilotEvalOracle.
package EvalSmoke {
    part def T { attribute m = 1 + (2 * 3); }
    calc c { 2 / 3 }
    part p : T[*];
    attribute recursive : ScalarValues::Integer = recursive + 1;
    attribute recovered : ScalarValues::Integer = 9;
}
