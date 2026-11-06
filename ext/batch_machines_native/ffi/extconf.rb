# frozen_string_literal: true
require 'mkmf'
require 'pathname'
require 'rb_sys/mkmf'
create_rust_makefile('batch_machines_native/batch_machines_native') do |extension|
  extension.ext_dir = Pathname(__dir__).relative_path_from(Pathname(Dir.pwd)).to_s
  extension.profile = ENV.fetch('RB_SYS_CARGO_PROFILE', :release).to_sym
end
makefile = File.join(Dir.pwd, 'Makefile')
File.write(makefile, File.read(makefile).gsub(
  /^RB_SYS_CARGO_MANIFEST_DIR \?=.*$/,
  "RB_SYS_CARGO_MANIFEST_DIR ?= #{File.expand_path(__dir__)}"
))
