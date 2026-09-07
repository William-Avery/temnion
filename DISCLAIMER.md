# Experimental software disclaimer

This document expands the warning in the [README](README.md) in plain language.
It does not replace or change the applicable license or any executed written
agreement.

## Experimental status and data risk

Temnion is publicly available experimental software. It is pre-production,
under active development, and is not a hardened database service. It may contain
defects that cause failures, data loss, data corruption, inaccurate data, or
other unexpected results. Recovery and durability features do not eliminate
these risks.

Use Temnion at your own risk. You assume the risk that data stored in, processed
by, or recovered with Temnion may be lost, corrupted, rendered inaccurate, or
made unusable. Do not use Temnion as the only copy of important data. Maintain
independent backups and test restoration before relying on any recovery process.

## Warranty and liability

For the public distribution under the GNU Affero General Public License v3.0,
Sections 15–17 of [`LICENSE`](LICENSE) are the controlling warranty and liability
terms. To the extent permitted by applicable law and unless otherwise stated or
agreed in writing, Temnion is provided **as is**, without warranty of any kind.
To that same extent, the copyright holders and other parties who modify or convey
the software are not liable for damages arising from its use or inability to be
used, including data loss, data corruption, inaccurate data, related losses, or
losses affecting third parties, even if advised that those damages were possible.

This paragraph is a plain-language summary, not an expansion or replacement of
the exact AGPL-3.0 terms. Applicable law controls where a warranty disclaimer or
liability limitation cannot take legal effect. A separately executed commercial
agreement may expressly provide different warranty, liability, support, or
service terms; [`COMMERCIAL_LICENSE.md`](COMMERCIAL_LICENSE.md) is only a notice
of availability and does not itself provide those terms.

## Operational limits

The [security policy](SECURITY.md) describes the current pre-production security
boundary and advises against using Temnion as the sole copy of important data.
The [storage documentation](docs/STORAGE.md) explains durability boundaries,
corruption handling, and precautions to take before recovery operations.
