"""
TC-2.10: globalIndex Decoding

Verifies correct decoding of the globalIndex bit field:
- Bit 64: mainnetFlag (1 = mainnet, 0 = rollup)
- Bits 32-63: rollupIndex
- Bits 0-31: localRootIndex
"""

import pytest
from tests.conftest import GlobalIndex


@pytest.mark.phase2
class TestGlobalIndexDecoding:
    """TC-2.10: globalIndex Decoding."""

    def test_mainnet_flag_extraction(self):
        """TC-2.10.1: Mainnet flag is correctly extracted from bit 64."""
        # mainnetFlag=1 case
        mainnet_gi = GlobalIndex.encode(
            mainnet_flag=True,
            rollup_index=0,
            local_root_index=0
        )
        assert mainnet_gi.mainnet_flag is True
        assert (mainnet_gi.raw >> 64) & 1 == 1

        # mainnetFlag=0 case
        rollup_gi = GlobalIndex.encode(
            mainnet_flag=False,
            rollup_index=0,
            local_root_index=0
        )
        assert rollup_gi.mainnet_flag is False
        assert (rollup_gi.raw >> 64) & 1 == 0

    def test_rollup_index_extraction(self):
        """TC-2.10.2: Rollup index is correctly extracted from bits 32-63."""
        test_cases = [0, 1, 100, 1000, 0xFFFFFFFF]

        for expected_rollup in test_cases:
            gi = GlobalIndex.encode(
                mainnet_flag=False,
                rollup_index=expected_rollup,
                local_root_index=0
            )
            assert gi.rollup_index == expected_rollup
            # Verify bit extraction
            extracted = (gi.raw >> 32) & 0xFFFFFFFF
            assert extracted == expected_rollup

    def test_local_root_index_extraction(self):
        """TC-2.10.3: Local root index is correctly extracted from bits 0-31."""
        test_cases = [0, 1, 42, 1000, 0xFFFFFFFF]

        for expected_local in test_cases:
            gi = GlobalIndex.encode(
                mainnet_flag=True,
                rollup_index=0,
                local_root_index=expected_local
            )
            assert gi.local_root_index == expected_local
            # Verify bit extraction
            extracted = gi.raw & 0xFFFFFFFF
            assert extracted == expected_local

    def test_encode_decode_roundtrip(self):
        """TC-2.10.4: Encode/decode roundtrip preserves all fields."""
        test_cases = [
            (True, 0, 0),
            (True, 0, 42),
            (False, 5, 100),
            (True, 1000, 500000),
            (False, 0xFFFFFFFF, 0xFFFFFFFF),
        ]

        for mainnet, rollup, local in test_cases:
            encoded = GlobalIndex.encode(mainnet, rollup, local)
            decoded = GlobalIndex.decode(encoded.raw)

            assert decoded.mainnet_flag == mainnet
            assert decoded.rollup_index == rollup
            assert decoded.local_root_index == local

    def test_bit_layout_matches_spec(self):
        """TC-2.10.5: Bit layout matches AggLayer spec."""
        # Per spec:
        # - Bit 64: mainnetFlag
        # - Bits 32-63: rollupIndex
        # - Bits 0-31: localRootIndex

        # Test specific known values
        gi = GlobalIndex.encode(
            mainnet_flag=True,    # bit 64 = 1
            rollup_index=5,       # bits 32-63 = 5
            local_root_index=42   # bits 0-31 = 42
        )

        # Expected raw value:
        # (1 << 64) | (5 << 32) | 42
        expected = (1 << 64) | (5 << 32) | 42
        assert gi.raw == expected

    def test_max_values(self):
        """TC-2.10.6: Maximum values for each field are handled."""
        gi = GlobalIndex.encode(
            mainnet_flag=True,
            rollup_index=0xFFFFFFFF,
            local_root_index=0xFFFFFFFF
        )

        decoded = GlobalIndex.decode(gi.raw)
        assert decoded.mainnet_flag is True
        assert decoded.rollup_index == 0xFFFFFFFF
        assert decoded.local_root_index == 0xFFFFFFFF

    def test_zero_global_index(self):
        """TC-2.10.7: Zero global index is valid (first rollup deposit)."""
        gi = GlobalIndex.decode(0)

        assert gi.mainnet_flag is False
        assert gi.rollup_index == 0
        assert gi.local_root_index == 0
        assert gi.raw == 0

    def test_rollup_vs_mainnet_distinction(self):
        """TC-2.10.8: Rollup and mainnet claims are properly distinguished."""
        # Same rollup/local indices but different mainnet flag
        rollup_claim = GlobalIndex.encode(False, 10, 100)
        mainnet_claim = GlobalIndex.encode(True, 10, 100)

        # They should have different raw values
        assert rollup_claim.raw != mainnet_claim.raw

        # But same non-flag fields
        assert rollup_claim.rollup_index == mainnet_claim.rollup_index
        assert rollup_claim.local_root_index == mainnet_claim.local_root_index

        # Different origins
        assert rollup_claim.mainnet_flag != mainnet_claim.mainnet_flag

    def test_unique_claim_identification(self):
        """TC-2.10.9: Global index uniquely identifies each claim."""
        # Two claims with any different field should have different indices
        claims = [
            GlobalIndex.encode(True, 0, 0),
            GlobalIndex.encode(True, 0, 1),
            GlobalIndex.encode(True, 1, 0),
            GlobalIndex.encode(False, 0, 0),
        ]

        raw_values = [c.raw for c in claims]
        # All should be unique
        assert len(set(raw_values)) == len(raw_values)
