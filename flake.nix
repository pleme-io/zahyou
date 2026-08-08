{
  description = "Zahyou (座標) — text coordinates: byte offsets <-> LSP line/UTF-16 positions";

  inputs.substrate.url = "github:pleme-io/substrate";

  outputs =
    { substrate, ... }:
    substrate.rust.library {
      src = ./.;
    };
}
