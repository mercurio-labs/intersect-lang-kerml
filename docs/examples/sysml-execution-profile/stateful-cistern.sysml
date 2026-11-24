package StatefulCistern {
    private import ISQ::*;
    private import SI::*;
    private import ScalarValues::*;

    item def Boost;
    item def Reset;

    state def FillModes {
        entry; then normal;
        state normal {
            attribute durationSeconds : Real = 2 * 5;
        }
        state boosted {
            attribute durationSeconds : Real = 4;
        }
        transition boost first normal accept signal : Boost then boosted;
        transition reset first boosted accept signal : Reset then normal;
    }

    part def Tank {
        exhibit state fillMode : FillModes;
    }
    part garden : Tank;

    analysis def FillSizing {
        subject tank : Tank;
        in attribute initial : VolumeValue = 1 [L];
        in attribute capacity : VolumeValue = 6 [L];
        in attribute fillTime : DurationValue;
        return 'flow' : VolumeFlowRateValue;
        assert constraint { capacity == initial + 'flow' * fillTime }
    }

    analysis sizeInlet : FillSizing {
        subject :>> tank = garden;
    }
}
