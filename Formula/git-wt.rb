class GitWt < Formula
  desc "Prepare new linked Git worktrees from a post-checkout hook"
  homepage "https://github.com/V1ncNet/git-worktree-init"
  version "0.2.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/V1ncNet/git-worktree-init/releases/download/v#{version}/git-wt-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "1c4e002fd54394ff012f9b7c32862fb36ecaccaecdccd0a0daba0693808df18b" # aarch64-apple-darwin
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/V1ncNet/git-worktree-init/releases/download/v#{version}/git-wt-#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "0bff96f808c528701699bfaab9f30ae7d7583a42a63ff48ebdf4e23500a6a769" # x86_64-unknown-linux-musl
    end
    on_arm do
      url "https://github.com/V1ncNet/git-worktree-init/releases/download/v#{version}/git-wt-#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "2974962a4ca9b1f931bf0608b19f414d820b93da98d68a6e2dd8fd0d745f6171" # aarch64-unknown-linux-musl
    end
  end

  def install
    bin.install "git-wt"
  end

  def caveats
    <<~EOS
      Register the hook once:

        git config --global hook.worktree-init.command 'f="#{opt_bin}/git-wt"; [ -x "$f" ] || exit 0; exec "$f" hook'
        git config --global --add hook.worktree-init.event post-checkout

      To use the harness, register its hook as well:

        git config --global hook.harness.command 'f="#{opt_bin}/git-wt"; [ -x "$f" ] || exit 0; exec "$f" harness-hook'
        git config --global --add hook.harness.event post-checkout
        git config --global --add hook.harness.event pre-push
        git config --global --add hook.harness.event reference-transaction
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/git-wt --version")
  end
end
