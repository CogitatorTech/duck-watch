{
  description = "DuckWatch: An observability tool for MotherDuck";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = f:
        nixpkgs.lib.genAttrs systems (system:
          let
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            };
          in
          f pkgs
        );
    in
    {
      devShells = forAllSystems (pkgs:
        let
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
        {
          default = pkgs.mkShell {
            name = "duck-watch-dev";

            packages = [
              rustToolchain
              pkgs.sqlx-cli
              pkgs.cargo-audit
              pkgs.nodejs_24
              pkgs.postgresql_18
              pkgs.docker-compose
              pkgs.gnumake
              pkgs.pre-commit
              pkgs.python3
              pkgs.uv
              pkgs.pkg-config
              pkgs.openssl
            ];

            DATABASE_URL = "postgres://postgres:postgres@localhost:5432/postgres?sslmode=disable";

            shellHook = ''
              echo "=========================================================="
              echo "  DuckWatch development environment"
              echo "  rust: $(rustc --version)"
              echo "  node: $(node --version)"
              echo "  psql: $(psql --version | cut -d' ' -f3)"
              echo ""
              echo "  make help       show the available targets"
              echo "  make docker-up  start PostgreSQL"
              echo "=========================================================="
            '';
          };
        });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
