// Authored nested/concurrent/cyclic qualification input, not Cameo recovery.
package M14Extended {
  item def Signal;
  action def Work { in item input : Signal; out item output : Signal; }
  action def Nested {
    action duplicate : Work;
    action other : Work;
    first start; then duplicate;
    first duplicate; then other;
    first other; then duplicate;
    first other; then done;
    flow transfer of Signal from duplicate.output to other.input;
  }
  action def Pipeline {
    action left : Nested;
    action right : Nested;
    first left; then right;
    first left; then right;
    first right; then left;
  }
  action pipeline : Pipeline;
  state def Concurrent parallel {
    state channelA {
      entry; then duplicate;
      state duplicate;
      state other;
      state complete :> States::StateAction::done;
      transition one first duplicate then other;
      transition two first duplicate then other;
      transition back first other then duplicate;
      transition repeat first other then other;
      transition finish first other then complete;
    }
    state channelB {
      entry; then duplicate;
      state duplicate;
      state other;
      transition one first duplicate then other;
      transition back first other then duplicate;
    }
  }
  state machine : Concurrent;
  private import StandardViewDefinitions::*;
  view actions : ActionFlowView { expose pipeline; }
  view states : StateTransitionView { expose machine; }
}
