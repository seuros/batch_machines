# frozen_string_literal: true
require 'bundler/gem_tasks'
require 'rake/testtask'
require 'rb_sys/extensiontask'
GEMSPEC = Gem::Specification.load('batch_machines.gemspec')
SUPPORTED_NATIVE_PLATFORMS = %w[
  arm64-darwin
  x86_64-darwin
  aarch64-linux
  x86_64-linux
  x86_64-linux-musl
].freeze

native_ext = RbSys::ExtensionTask.new('batch_machines_native', GEMSPEC) do |ext|
  ext.lib_dir = 'lib/batch_machines_native'
  ext.tmp_dir = 'tmp/rb_sys'
  ext.cross_platform = SUPPORTED_NATIVE_PLATFORMS if ENV.key?('RUBY_TARGET')
end

# A single-version cross build stages its binary at the host binary's path,
# which the gem package depends on, so packaging would also compile for the
# host and fail where the host linker can't target the platform.
if ENV.key?('RUBY_TARGET')
  host_binary = "#{native_ext.lib_dir}/#{native_ext.name}.#{RbConfig::CONFIG['DLEXT']}"
  Rake::Task[host_binary].clear_prerequisites if Rake::Task.task_defined?(host_binary)
end
Rake::TestTask.new(:test => :compile) do |test|
  test.libs << 'lib'
  test.pattern = 'test/**/*_test.rb'
end
task :rbs do
  sh 'rbs', '-I', 'sig', 'validate'
end
task default: %i[test rbs]

Dir[File.join(__dir__, 'tasks/*.rake')].sort.each { |f| load f }
