package LunarRover {
    private import ScalarValues::*;
    private import RoverTypes::*;

    doc /* A compact exploration vehicle for the lunar surface. */
    part def Rover {
        attribute mass : Real;
        attribute operational : Boolean;
        part chassis : Chassis;
        part power : PowerSystem;
        part wheels : Wheel[6];
        connect power.supply to chassis.power;
    }

    part def Chassis {
        port power : ~EnergyPort;
        attribute material : String;
    }

    part def PowerSystem {
        port supply : EnergyPort;
        attribute capacity : Real;
        attribute voltage : Real;
    }

    part def Wheel {
        attribute diameter : Real;
    }

    port def EnergyPort {
        out item energy;
    }

    part rover : Rover;
}
