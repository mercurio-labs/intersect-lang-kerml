// Each view is a projection of the same accepted declarations.
package M13Views {
  private import StandardViewDefinitions::*;
  private import Views::*;
  view def TreeBase :> GeneralView { render rendering choice :> Views::asTreeDiagram; }
  view def WireBase :> GeneralView { render asInterconnectionDiagram; }
  view def Replaced :> TreeBase { render asInterconnectionDiagram :>> TreeBase::choice; }
  alias TreeAlias for Views::asTreeDiagram;
  view compartments : GeneralView { expose M13Structure::Derived; expose M13Structure::Base; expose M13Structure::Module; expose M13Structure::Leaf; expose M13Structure::vehicle; }
  view tree : TreeBase { expose M13Structure::vehicle; expose M13Structure::spare; }
  view wiring : WireBase { expose M13Structure::vehicle; expose M13Structure::spare; }
  view explicit : GeneralView { render asTreeDiagram; expose M13Structure::vehicle; }
  view replaced : Replaced { expose M13Structure::vehicle; }
  view aliasSelection : GeneralView { render TreeAlias; expose M13Structure::vehicle; }
  view incompatibleRoots : TreeBase { expose M13Structure::Derived; }
  package Spoof {
    package StandardViewDefinitions { view def GeneralView; }
    package Views { rendering asTreeDiagram; }
  }
  view lookalikeDefault : Spoof::StandardViewDefinitions::GeneralView { expose M13Structure::vehicle; }
  view lookalikeRendering : GeneralView { render Spoof::Views::asTreeDiagram; expose M13Structure::vehicle; }
}
