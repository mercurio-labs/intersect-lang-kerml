`stdlib-usage.kpar` is a deterministic fixture written independently with
Python's standard `zipfile` module (DEFLATE). It carries KerML §10.3 project
and metadata records and an unconstrained usage of the OMG Semantic Library
IRI. The bridge test uses a small synthetic prepared library: it checks
acquisition/catalog/lock integration, not KerML parsing or semantic qualification.
