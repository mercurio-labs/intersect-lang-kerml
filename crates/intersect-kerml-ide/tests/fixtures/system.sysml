package DriveSystem {
    port def PowerPort {
        out item energy;
    }

    part def Battery {
        port supply : PowerPort;
    }
    part def Motor {
        port power : ~PowerPort;
    }
    part def Drive {
        part battery : Battery;
        part motor : Motor;
        connect battery.supply to motor.power;
    }

    part drive : Drive;
}
