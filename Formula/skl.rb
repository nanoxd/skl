class Skl < Formula
  desc "Keep agent skills in sync across coding agents and machines"
  homepage "https://github.com/nanoxd/skl"
  license "MIT"

  depends_on "git"

  on_macos do
    on_arm do
      url "https://github.com/nanoxd/skl/releases/download/v0.4.0/skl-aarch64-apple-darwin.tar.gz"
      sha256 "c223db024d7fa12681e98363d05ced687dda8873a0a6a66f9516d898ce22d2fc"
    end
    on_intel do
      url "https://github.com/nanoxd/skl/releases/download/v0.4.0/skl-x86_64-apple-darwin.tar.gz"
      sha256 "2715392083ff23bdf4e287917b8b6fda3a6c05f7efe01d3934e6840918565c83"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/nanoxd/skl/releases/download/v0.4.0/skl-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "299a396c190d2ffb1d276f405ca4717c903360f63e5c9dab258140f400308d6f"
    end
    on_intel do
      url "https://github.com/nanoxd/skl/releases/download/v0.4.0/skl-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ff3a98b9505f9e4db74887315827f5829ef32cc188d170826c409bf067c9fa41"
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
