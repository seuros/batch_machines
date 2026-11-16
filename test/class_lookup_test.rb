# frozen_string_literal: true

require 'minitest/autorun'
require 'open3'
require 'rbconfig'

# The class of the value the extension wraps (Batch) is fixed when it loads.
# Nothing a caller does later, redefining the constant or `Object.const_get`,
# reaches a batch being built. Each case runs in a fresh process: the class is
# fixed once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  PRELUDE = <<~RUBY
    require 'batch_machines'
    BM = BatchMachines
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: a Batch constant redefined before the first `new` made the value an
  # instance of the new class, without #push.
  def test_a_redefined_batch_constant_does_not_change_the_batch_class
    assert_equal 'true buffered 1', isolated(<<~RUBY)
      original = BM::Batch
      BM.send(:remove_const, :Batch)
      BM.const_set(:Batch, Class.new)
      batch = original.new(2, 10)
      puts [batch.instance_of?(original), batch.push(1), batch.pending].join(' ')
    RUBY
  end

  # Bug: building the first batch ran `Object.const_get`, Ruby code in the
  # middle of a native constructor.
  def test_building_a_batch_runs_no_const_get
    assert_equal '0 Batch', isolated(<<~RUBY)
      hits = 0
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        super(*args)
      end
      batch = BM::Batch.new(2, 10)
      puts [hits, batch.class.name.split('::').last].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal 'buffered 1', isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran in a constructor' }
      batch = BM::Batch.new(2, 10)
      puts [batch.push(1), batch.pending].join(' ')
    RUBY
  end
end
