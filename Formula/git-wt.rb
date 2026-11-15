class GitWt < Formula
  desc "Prepare new linked Git worktrees from a post-checkout hook"
  homepage "https://github.com/V1ncNet/git-worktree-init"
  version "0.2.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/V1ncNet/git-worktree-init/releases/download/v#{version}/git-wt-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "af15088ce075be9c0a6ce69b45e152a7d87e9f95638733ae1fa1b3b662ab7ccb" # aarch64-apple-darwin
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/V1ncNet/git-worktree-init/releases/download/v#{version}/git-wt-#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "d7bf791990aaa59d78c20488d8b780e5a460fdcf7edfd2e2d7b9296663157615" # x86_64-unknown-linux-musl
    end
    on_arm do
      url "https://github.com/V1ncNet/git-worktree-init/releases/download/v#{version}/git-wt-#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "0dfda5546db1eb37ba8106d6790593f3ddf54a19df5a9266c027e0113a7d32d0" # aarch64-unknown-linux-musl
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
