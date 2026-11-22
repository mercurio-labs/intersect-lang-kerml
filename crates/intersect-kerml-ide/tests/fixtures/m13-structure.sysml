// Authored M13 qualification example; not the Cameo Simple Vehicle source.
package M13Structure {
  port def SignalPort;
  part def Leaf { port io : ~SignalPort; attribute mass; }
  part def Module { part leaf : Leaf; port io : SignalPort; }
  part def Base { part retained : Module; part replaced : Module; }
  part def Derived :> Base { part chosen : Module redefines Base::replaced; }
  part vehicle : Derived { connect retained.io to chosen.io; }
  part spare : Derived;
}
