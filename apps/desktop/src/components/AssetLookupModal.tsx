import { Modal } from "./Modal";
import { Button } from "../ui";
import { AssetLookupTools } from "../views/AssetLookupTools";

interface AssetLookupModalProps {
  open: boolean;
  onClose: () => void;
}

export function AssetLookupModal({ open, onClose }: AssetLookupModalProps) {
  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Extract asset lookup"
      subtitle="Decode mobys, ties, textures, shaders and more directly from a V2 (R2 / R3 / RCFFA) assetlookup.dat"
      size="lg"
      footer={
        <Button onClick={onClose}>Close</Button>
      }
    >
      <AssetLookupTools />
    </Modal>
  );
}
