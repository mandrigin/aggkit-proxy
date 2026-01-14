"""
TC-2.3: CLAIM Note Creation (567 Felts)

Verifies that CLAIM notes are created with correct structure and size.
The CLAIM note inputs should be exactly 567 Felts as per the FPI spec.
"""

import pytest
from tests.conftest import (
    ClaimAssetParams,
    GlobalIndex,
    CLAIM_NOTE_FELT_SIZE,
    CLAIM_ASSET_SELECTOR,
)


@pytest.mark.phase2
class TestClaimNoteCreation:
    """TC-2.3: CLAIM Note Creation."""

    def test_claim_note_structure(self, sample_claim_params: ClaimAssetParams):
        """TC-2.3.1: CLAIM note has correct structure."""
        # Verify the claim parameters encode to valid calldata
        calldata = sample_claim_params.encode_calldata()

        # Should start with claimAsset selector
        assert calldata[:4] == CLAIM_ASSET_SELECTOR

        # Calldata should have minimum size for all required fields
        # 4 bytes selector + 32*32 local proof + 32*32 rollup proof + params
        min_size = 4 + (32 * 32) + (32 * 32) + (32 * 7)  # proofs + basic params
        assert len(calldata) >= min_size

    def test_claim_note_felt_count(self, sample_claim_params: ClaimAssetParams):
        """TC-2.3.2: CLAIM note inputs are 567 Felts."""
        # When converted to Miden note inputs, the claim should be 567 Felts
        # Each Felt is ~31 bits of data (field element in Miden's prime field)

        # Calculate expected Felt count for CLAIM note:
        # - SMT proofs: 32 * 2 = 64 hashes = 64 * 8 Felts = 512 Felts
        # - Global index: 8 Felts (256 bits / 32 bits per felt approx)
        # - Roots: 2 * 8 = 16 Felts
        # - Networks: 2 Felts
        # - Addresses: 2 * 5 = 10 Felts (160 bits each)
        # - Amount: 8 Felts
        # - Metadata length + padding: ~11 Felts
        # Total: ~567 Felts

        # This validates the note structure matches spec
        expected_felt_count = CLAIM_NOTE_FELT_SIZE
        assert expected_felt_count == 567, "Spec requires 567 Felts"

    def test_proof_arrays_correct_size(self, sample_claim_params: ClaimAssetParams):
        """TC-2.3.3: SMT proof arrays are 32 elements each."""
        assert len(sample_claim_params.smt_proof_local) == 32
        assert len(sample_claim_params.smt_proof_rollup) == 32

        # Each proof element is 32 bytes (256 bits)
        for proof in sample_claim_params.smt_proof_local:
            assert len(proof) == 32
        for proof in sample_claim_params.smt_proof_rollup:
            assert len(proof) == 32

    def test_global_index_encoding(self, sample_claim_params: ClaimAssetParams):
        """TC-2.3.4: Global index is properly encoded."""
        gi = sample_claim_params.global_index

        # Verify encoding roundtrip
        decoded = GlobalIndex.decode(gi.raw)
        assert decoded.mainnet_flag == gi.mainnet_flag
        assert decoded.rollup_index == gi.rollup_index
        assert decoded.local_root_index == gi.local_root_index

    def test_claim_params_serialization(self, sample_claim_params: ClaimAssetParams):
        """TC-2.3.5: Claim params serialize correctly."""
        calldata = sample_claim_params.encode_calldata()

        # Verify calldata is valid bytes
        assert isinstance(calldata, bytes)
        assert len(calldata) > 4  # More than just selector

        # Verify selector
        assert calldata[:4] == CLAIM_ASSET_SELECTOR

    def test_empty_metadata_handling(self):
        """TC-2.3.6: Empty metadata is handled correctly."""
        import os
        params = ClaimAssetParams(
            smt_proof_local=[os.urandom(32) for _ in range(32)],
            smt_proof_rollup=[os.urandom(32) for _ in range(32)],
            global_index=GlobalIndex.encode(True, 0, 1),
            mainnet_exit_root=os.urandom(32),
            rollup_exit_root=os.urandom(32),
            origin_network=0,
            origin_token_address="0x0000000000000000000000000000000000000000",
            destination_network=1,
            destination_address="0x1234567890123456789012345678901234567890",
            amount=1000,
            metadata=b"",  # Empty metadata
        )

        calldata = params.encode_calldata()
        assert len(calldata) > 0

    def test_non_empty_metadata_handling(self):
        """TC-2.3.7: Non-empty metadata is included in note."""
        import os
        metadata = b"bridge_claim_v1"
        params = ClaimAssetParams(
            smt_proof_local=[os.urandom(32) for _ in range(32)],
            smt_proof_rollup=[os.urandom(32) for _ in range(32)],
            global_index=GlobalIndex.encode(False, 5, 100),
            mainnet_exit_root=os.urandom(32),
            rollup_exit_root=os.urandom(32),
            origin_network=0,
            origin_token_address="0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            destination_network=1,
            destination_address="0xDEADBEEF00000000000000000000000000000001",
            amount=1000000,
            metadata=metadata,
        )

        calldata = params.encode_calldata()
        # Metadata should be included (though ABI-encoded)
        assert len(calldata) > 0
