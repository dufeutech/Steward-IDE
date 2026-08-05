"""Publisher side of the asset-pack contract.

`core/` holds pure stages (manifest generation and verification, repository
layout planning). `adapters/` holds everything that touches an external system
(the npm registry, the JSON Schema validator, `tuftool`, the filesystem).
Dependencies point inward: core imports no adapter.
"""


class PackError(Exception):
    """Any publisher-side failure worth reporting to the operator verbatim."""
