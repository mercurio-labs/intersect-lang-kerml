// Authored M14 canonical policy cases; complete scene integration follows.
package M14Profiles {
  private import StandardViewDefinitions::*;
  private import Views::*;
  action root;
  state status;
  alias ActionAlias for StandardViewDefinitions::ActionFlowView;
  view def WireAction :> ActionFlowView { render asInterconnectionDiagram; }
  view def TreeAction :> ActionFlowView { render rendering choice :> Views::asTreeDiagram; }
  view def FixedAction :> TreeAction { render asInterconnectionDiagram :>> TreeAction::choice; }
  view def Mixed :> ActionFlowView, StateTransitionView;
  view defaultAction : ActionFlowView { expose root; }
  view defaultState : StateTransitionView { expose status; }
  view explicitAction : ActionFlowView { render asInterconnectionDiagram; expose root; }
  view explicitState : StateTransitionView { render asInterconnectionDiagram; expose status; }
  view inheritedAction : WireAction { expose root; }
  view lawfulAction : FixedAction { expose root; }
  view aliasAction : ActionAlias { expose root; }
  view wrongTree : ActionFlowView { render asTreeDiagram; expose root; }
  view mixed : Mixed { expose root; }
  package Spoof { view def ActionFlowView; rendering asInterconnectionDiagram; }
  view lookalike : Spoof::ActionFlowView { render Views::asInterconnectionDiagram; expose root; }
  view fakeDefault : Spoof::ActionFlowView { expose root; }
  view fakeRendering : ActionFlowView { render Spoof::asInterconnectionDiagram; expose root; }
}
