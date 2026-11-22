package DriveViews {
    private import StandardViewDefinitions::*;
    private import Views::*;

    viewpoint def StructurePerspective {
        doc /* Review the drive's components and power interfaces. */
    }

    view def DefinitionOverview :> GeneralView {
        viewpoint concerns : StructurePerspective;
        // Structured compartments use the documented GeneralView default.
    }
    view def InternalStructure :> InterconnectionView {
        viewpoint concerns : StructurePerspective;
        render asInterconnectionDiagram;
    }

    view definitions : DefinitionOverview {
        expose DriveSystem::PowerPort;
        expose DriveSystem::Battery;
        expose DriveSystem::Motor;
        expose DriveSystem::Drive;
        expose DriveSystem::drive;
    }
    view structure : InternalStructure {
        expose DriveSystem::drive;
    }
}
