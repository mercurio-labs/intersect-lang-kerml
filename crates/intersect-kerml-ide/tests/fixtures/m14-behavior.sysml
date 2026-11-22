// Authored M14 semantic feasibility input, not recovered Cameo source.
package M14Behavior {
  item def Signal;
  action def Work {
    in item input : Signal;
    out item output : Signal;
  }
  action def Pipeline {
    first start;
    then fork split;
      then firstStep;
      then secondStep;
    action firstStep : Work;
    then settle;
    action secondStep : Work;
    then settle;
    join settle;
    then finish;
    action finish;
    then done;
    flow transfer of Signal from firstStep.output to secondStep.input;
  }
  action pipeline : Pipeline;
  action enterNormal;
  action monitor;
  action leaveNormal;
  action react;
  state def Machine {
    entry; then normal;
    state normal {
      entry enterNormal;
      do monitor;
      exit leaveNormal;
    }
    state degraded;
    state maintenance;
    transition heat first normal accept signal : Signal if true do react then degraded;
    transition recover first degraded accept signal : Signal then normal;
    transition service first normal then maintenance;
    transition again first maintenance then maintenance;
  }
  state machine : Machine;
  private import StandardViewDefinitions::*;
  view actions : ActionFlowView { expose pipeline; }
  view states : StateTransitionView { expose machine; }
}
