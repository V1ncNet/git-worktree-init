class GitWt < Formula
  desc "Prepare new linked Git worktrees from a post-checkout hook"
  homepage "https://github.com/V1ncNet/git-worktree-init"
  version "0.2.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/V1ncNet/git-worktree-init/releases/download/v#{version}/git-wt-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "ff77e2f0f8b7c5c6c46e5f4ef9324e919698a1d9f5c1b7c8bad889f593517b48" # aarch64-apple-darwin
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/V1ncNet/git-worktree-init/releases/download/v#{version}/git-wt-#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "36b3d84de15d0851bbc39602b49e0cec77ba2f1294dbb3a90f903e5cec13bcca" # x86_64-unknown-linux-musl
    end
    on_arm do
      url "https://github.com/V1ncNet/git-worktree-init/releases/download/v#{version}/git-wt-#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "9f77ea978f10842c4822fc6e58269653db9bc546e19a5c6c02004c6521cb078d" # aarch64-unknown-linux-musl
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
