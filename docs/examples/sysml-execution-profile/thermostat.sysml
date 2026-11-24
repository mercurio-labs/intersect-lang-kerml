package Thermostat {
    private import ScalarValues::*;

    item def TooHot;
    item def TooCool;

    attribute initialTemperatureC : Real = 20.0;
    attribute shutoffTemperatureC : Real = 40.0;
    attribute restartTemperatureC : Real = 38.0;

    state def HeaterModes {
        entry; then heating;
        state heating {
            attribute heaterPowerW : Real = 120.0;
        }
        state idle {
            attribute heaterPowerW : Real = 0.0;
        }
        transition shutoff first heating accept signal : TooHot then idle;
        transition restart first idle accept signal : TooCool then heating;
    }

    part def ThermalEnclosure {
        exhibit state controller : HeaterModes;
    }
    part cabin : ThermalEnclosure;

    analysis def HeatBalance {
        subject enclosure : ThermalEnclosure;
        in attribute temperatureC : Real;
        in attribute powerW : Real;
        in attribute ambientC : Real = 20.0;
        in attribute lossWPerC : Real = 5.0;
        in attribute capacityJPerC : Real = 1200.0;
        return rateCPerSecond : Real;
        assert constraint {
            capacityJPerC * rateCPerSecond == powerW - lossWPerC * (temperatureC - ambientC)
        }
    }

    analysis temperatureRate : HeatBalance {
        subject :>> enclosure = cabin;
    }
}
