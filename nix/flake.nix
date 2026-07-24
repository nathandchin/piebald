{
  description = "A hobby Game Boy (DMG) emulator";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustup
            glfw
            cmake
            clang
            wayland
            wayland-scanner
            libxkbcommon
            libffi
            libx11
            libxrandr
            libxinerama
            libxcursor
            libxi
            libGL
            pkg-config
          ];

          LD_LIBRARY_PATH = with pkgs; lib.makeLibraryPath [
            libxkbcommon
            libGL
          ];
          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
      };
    }
  );
}
