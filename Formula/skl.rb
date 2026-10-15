class Skl < Formula
  desc "Keep agent skills in sync across coding agents and machines"
  homepage "https://github.com/nanoxd/skl"
  license "MIT"

  depends_on "git"

  on_macos do
    on_arm do
      url "https://github.com/nanoxd/skl/releases/download/v0.2.0/skl-aarch64-apple-darwin.tar.gz"
      sha256 "f3205c74a775d7e186c3b774fcf22f43db1bd95ed2f808e9da578a7a1044513a"
    end
    on_intel do
      url "https://github.com/nanoxd/skl/releases/download/v0.2.0/skl-x86_64-apple-darwin.tar.gz"
      sha256 "2d9bf079f9a98a1cd2e5f60804ca88123e1830f5cfc133651c268c5dfe5a68ed"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/nanoxd/skl/releases/download/v0.2.0/skl-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "34b8fc70ecd3218535e9a3ce204d0fcaf0f1c4716d3d666d771d978b42ad3d9c"
    end
    on_intel do
      url "https://github.com/nanoxd/skl/releases/download/v0.2.0/skl-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "e5e02a37b64ed491352f3517d9890b8f1e3ec72248466c54acf2beeedb357435"
    end
  end

  def install
    bin.install "skl"
  end

  test do
    ENV["SKL_CONFIG_DIR"] = (testpath/"config").to_s
    ENV["SKL_DATA_DIR"] = (testpath/"data").to_s
    ENV.delete("SKL_PROFILE")
    (testpath/"local/SKILL.md").write("---\nname: local\n---\nA local test skill.\n")

    assert_match version.to_s, shell_output("#{bin}/skl --version")
    system bin/"skl", "add", testpath/"local", "--agent", (testpath/"skills").to_s
    assert_equal testpath/"local", (testpath/"skills/local").realpath
    system bin/"skl", "sync", "--locked"
    system bin/"skl", "remove", "local"
    refute_path_exists testpath/"skills/local"
    assert_path_exists testpath/"local/SKILL.md"
  end
end
