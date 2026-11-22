package RoverViews {
    private import StandardViewDefinitions::*;
    private import Views::*;

    viewpoint def StructuralConcerns {
        doc /* Show the rover's composition and power interfaces for design review. */
    }

    view def Overview :> GeneralView {
        viewpoint concerns : StructuralConcerns;
    }
    view def Structure :> InterconnectionView {
        viewpoint concerns : StructuralConcerns;
        render asInterconnectionDiagram;
    }

    view overview : Overview {
        expose LunarRover::Rover;
        expose LunarRover::Chassis;
        expose LunarRover::PowerSystem;
        expose LunarRover::Wheel;
        expose LunarRover::EnergyPort;
        expose LunarRover::rover;
    }
    view structure : Structure {
        expose LunarRover::rover;
    }
}
