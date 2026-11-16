"""Static usage control; evaluated by basedpyright, never run as a test."""
from typing import assert_type
from intersect_kerml import Engine, SourceLocation
from intersect_kerml.sysml import PartDefinition

def inspect(engine: Engine) -> None:
    project = engine.new_project()
    model = project.analyze().require_model()
    for part in model.elements(PartDefinition):
        assert_type(part.declared_name, str | None)
        assert_type(part.source_location, SourceLocation)
        assert_type(part.id, str)
