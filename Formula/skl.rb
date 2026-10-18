class Skl < Formula
  desc "Keep agent skills in sync across coding agents and machines"
  homepage "https://github.com/nanoxd/skl"
  license "MIT"

  depends_on "git"

  on_macos do
    on_arm do
      url "https://github.com/nanoxd/skl/releases/download/v0.3.0/skl-aarch64-apple-darwin.tar.gz"
      sha256 "9671d8dc5f15616be35075f6d567a41b113717916ef5b2b0f98ae51650504a9e"
    end
    on_intel do
      url "https://github.com/nanoxd/skl/releases/download/v0.3.0/skl-x86_64-apple-darwin.tar.gz"
      sha256 "90a9a1933157c234fd6aaccde49bfa8ceb83cdafe3639e4f0d9b3da6ca6a737e"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/nanoxd/skl/releases/download/v0.3.0/skl-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "08f87dc16eb645643b918bc276219a49bf573de8f75ca8935c7a30fc04202ab4"
    end
    on_intel do
      url "https://github.com/nanoxd/skl/releases/download/v0.3.0/skl-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "c2d5ac9c43e77db4c900a27ac81d9afe7d0e597e68d797316d6a0f69c5232e43"
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
