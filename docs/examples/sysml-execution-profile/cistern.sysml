package CisternAnalysis {
    private import ISQ::*;
    private import SI::*;

    part def Tank;
    part garden : Tank;
    part reserve : Tank;

    analysis def FillSizing {
        subject tank : Tank;
        in attribute initial : VolumeValue = 1 [L];
        in attribute capacity : VolumeValue = (2 + 4) [L];
        in attribute fillTime : DurationValue;
        return 'flow' : VolumeFlowRateValue;
        assert constraint { capacity == initial + 'flow' * fillTime }
    }

    analysis sizeInlet : FillSizing {
        subject :>> tank = garden;
    }

    analysis sizeSlow : FillSizing {
        subject :>> tank = reserve;
    }
}
