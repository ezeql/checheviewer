class Checheviewer < Formula
  desc "Fast, keyboard-driven image viewer for macOS, inspired by IrfanView"
  homepage "https://github.com/ezeql/checheviewer"
  url "https://github.com/ezeql/checheviewer.git", tag: "v0.1.0"
  head "https://github.com/ezeql/checheviewer.git", branch: "main"

  depends_on "rust" => :build
  depends_on :macos

  def install
    system "make", "bundle"
    prefix.install "target/CheCheViewer.app"
    bin.write_exec_script prefix/"CheCheViewer.app/Contents/MacOS/checheviewer"
  end

  def caveats
    <<~EOS
      To list CheCheViewer in Finder's "Open With" menu, launch it once:
        open #{opt_prefix}/CheCheViewer.app
    EOS
  end

  test do
    assert_path_exists prefix/"CheCheViewer.app/Contents/Info.plist"
    assert_predicate bin/"checheviewer", :executable?
  end
end
