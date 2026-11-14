# frozen_string_literal: true

require 'minitest/autorun'
require 'batch_machines'

# A FeedbackMachines::Pid-shaped controller: proportional, gain 1, output
# clamped to 0..max. It answers what the sibling gem's `step` answers.
class ProportionalDuck
  attr_reader :steps

  def initialize(max)
    @max = max
    @steps = 0
  end

  def step(error, elapsed)
    return :not_finite unless error.to_f.finite?
    return :invalid_duration if elapsed.zero?
    return :overflow if error.abs > 1.0e300

    @steps += 1
    error.to_f.clamp(0.0, @max)
  end
end

class BatchMachinesTest < Minitest::Test
  Batch = BatchMachines::Batch
  Flush = BatchMachines::Flush
  SIZE = Flush.new(:size)
  DEADLINE = Flush.new(:deadline)
  READERS = %i[capacity max_wait pending pending_cost target deadline].freeze

  def snapshot(batch) = READERS.map { |reader| batch.public_send(reader) }

  def test_size_full_flush_and_deadline
    b = Batch.new(2, 10)
    assert_equal [2, 10, 0, 0, 2, nil], snapshot(b)
    assert_nil b.poll(0)
    assert_equal :buffered, b.push(1)
    assert_equal 11, b.deadline
    assert_equal SIZE, b.push(2)
    assert_equal :full, b.push(3)
    assert_equal 2, b.pending
    assert_equal 2, b.flush(3)
    assert_equal 0, b.flush(3)
    assert_nil b.deadline
    assert_equal :buffered, b.push(4)
    assert_nil b.poll(13)
    assert_equal :deadline, b.poll(14)
    assert_equal DEADLINE, b.push(14) # counted, and deadline wins over size
    assert_equal 2, b.flush(14)
    assert_equal 0, b.pending
  end

  def test_weighted_pushes_spend_cost
    b = Batch.new(10, 5)
    assert_equal :buffered, b.push_weighted(0, 4)
    assert_equal :full, b.push_weighted(1, 7)
    assert_equal SIZE, b.push_weighted(1, 6)
    assert_equal [2, 10], [b.pending, b.pending_cost]
    assert_equal :full, b.push(1)
    assert_equal 2, b.flush(2)
    assert_equal 0, b.pending_cost
  end

  def test_refusals_are_symbols_and_change_nothing
    b = Batch.new(3, 10)
    assert_equal :buffered, b.push_weighted(5, 2)
    before = snapshot(b)
    assert_equal :time_regression, b.push(4)
    assert_equal :time_regression, b.push_weighted(4, 1)
    assert_equal :time_regression, b.poll(4)
    assert_equal :time_regression, b.flush(4)
    assert_equal :full, b.push_weighted(99, 2)
    assert_equal :not_finite, b.retarget(Float::NAN)
    assert_equal before, snapshot(b)
    # The refused push at 99 observed nothing: 5 is still the latest instant.
    assert_nil b.poll(5)
  end

  # The core's push refuses `Overflow` when `now + max_wait` does not fit a
  # Duration. An Integer nanosecond instant tops out far below that, so no
  # Ruby push can reach it: the latest instant plus the longest wait is fine.
  def test_push_overflow_is_out_of_reach
    last = (2**64) - 1
    b = Batch.new(2, last)
    assert_equal :buffered, b.push(last)
    assert_equal 2 * last, b.deadline
    assert_equal SIZE, b.push(last)
    assert_equal 2, b.flush(last)
  end

  def test_retarget_truncates_and_clamps
    b = Batch.new(5, 10)
    assert_equal 2, b.retarget(2.9)
    assert_equal 1, b.retarget(-100)
    assert_equal 5, b.retarget(1.0e300)
    assert_equal :not_finite, b.retarget(Float::INFINITY)
    assert_equal 5, b.target
    assert_raises(TypeError) { b.retarget('2') }
    assert_raises(TypeError) { b.retarget(nil) }
  end

  def test_adapt_steps_the_callers_controller
    b = Batch.new(5, 10)
    pid = ProportionalDuck.new(100.0)
    assert_equal :buffered, b.push(0)
    assert_equal 2, b.adapt(pid, 2.9, 1_000_000_000)
    assert_equal 2, b.target
    assert_equal SIZE, b.push(1)
    assert_equal 5, b.adapt(pid, 100.0, 1)
    assert_equal 1, b.adapt(pid, -100.0, 1)
    assert_equal :size, b.poll(9)
    assert_equal 5, b.adapt(pid, 5, 1)
    assert_equal 4, pid.steps
    assert_nil b.poll(9)
    assert_equal :deadline, b.poll(10) # adapting never moves the deadline
  end

  def test_adapt_passes_the_controllers_refusals_and_exceptions_through
    b = Batch.new(5, 10)
    pid = ProportionalDuck.new(5.0)
    before = snapshot(b)
    assert_equal :not_finite, b.adapt(pid, Float::NAN, 1)
    assert_equal :invalid_duration, b.adapt(pid, 1.0, 0)
    assert_equal :overflow, b.adapt(pid, 1.0e308, 1)
    assert_equal 0, pid.steps
    # A controller that raises has been asked once and the batch is untouched.
    frozen = Object.new
    frozen.define_singleton_method(:step) { |*| raise FrozenError, "can't modify frozen controller" }
    assert_raises(FrozenError) { b.adapt(frozen, 5.0, 1) }
    assert_equal before, snapshot(b)
  end

  def test_adapt_rejects_malformed_input_before_stepping
    b = Batch.new(5, 10)
    pid = ProportionalDuck.new(5.0)
    assert_raises(TypeError) { b.adapt(pid, '1', 1) }
    assert_raises(TypeError) { b.adapt(pid, 1.0, 1.5) }
    assert_raises(RangeError) { b.adapt(pid, 1.0, -1) }
    assert_raises(NoMethodError) { b.adapt(Object.new, 1.0, 1) }
    assert_equal 0, pid.steps
    [:stale, nil, 3, true].each do |answer|
      odd = Object.new
      odd.define_singleton_method(:step) { |*| answer }
      assert_raises(answer == :stale ? RangeError : TypeError) { b.adapt(odd, 1.0, 1) }
    end
    assert_equal 5, b.target
  end

  def test_flush_is_a_data_value_validated_like_the_core
    flush = Batch.new(1, 10).push(0)
    assert_kind_of Data, flush
    assert_equal [:reason], Flush.members
    assert_equal :size, flush.reason
    assert_equal DEADLINE, Flush.new(reason: :deadline)
    assert_raises(RangeError) { Flush.new(:nope) }
    assert_raises(RangeError) { SIZE.with(reason: :buffered) }
    ['size', nil, 1, 1.5].each { |bad| assert_raises(TypeError) { Flush.new(bad) } }
    assert_raises(ArgumentError) { Flush.new }
    assert_raises(NoMethodError) { BatchMachines.__flush(:size) }
  end

  # Flush is resolved once at load: removing or swapping the constant later
  # changes nothing the batch builds.
  def test_the_flush_class_is_resolved_at_load
    b = Batch.new(1, 10)
    flush = BatchMachines.send(:remove_const, :Flush)
    begin
      BatchMachines.const_set(:Flush, Data.define(:reason))
      assert_instance_of flush, b.push(0)
    ensure
      BatchMachines.send(:remove_const, :Flush)
      BatchMachines.const_set(:Flush, flush)
    end
  end

  def test_flush_is_sealed
    assert_raises(NoMethodError) { Flush.allocate }
    assert_raises(TypeError) { Class.new(Flush) { def initialize(**) = nil } }
    # A named subclass raises too, but Ruby binds the constant before
    # `inherited` runs, so the (unusable for the batch) constant leaks.
    leak = 'class LeakedFlush < BatchMachines::Flush; end'
    assert_raises(TypeError) { self.class.class_eval(leak, __FILE__, __LINE__) }
    assert self.class.const_defined?(:LeakedFlush, false)
  end

  # The binding never loses work: a Flush.new that raises (anything failing
  # while the answer is built) counts nothing, and the next push is counted.
  def test_a_push_that_cannot_build_its_answer_counts_nothing
    b = Batch.new(1, 10)
    replacing_flush_new(->(*) { raise IOError, 'the answer could not be built' }) do
      assert_raises(IOError) { b.push(0) }
    end
    assert_equal [0, 0, nil], [b.pending, b.pending_cost, b.deadline]
    assert_equal SIZE, b.push(0)
  end

  # An answer whose construction itself changed the batch is not applied
  # over that change.
  def test_a_push_whose_answer_changes_the_batch_is_not_applied_over_it
    b = Batch.new(2, 10)
    assert_equal :buffered, b.push(0)
    replacing_flush_new(->(*args) { b.flush(1) && Flush.__new(*args) }) do
      assert_raises(RuntimeError) { b.push(1) }
    end
    assert_equal [0, 0, nil], [b.pending, b.pending_cost, b.deadline]
  end

  # An answer whose construction froze the batch raises FrozenError and the
  # push is not applied.
  def test_a_push_whose_answer_freezes_the_batch_is_not_applied
    b = Batch.new(2, 10)
    assert_equal :buffered, b.push(0)
    before = snapshot(b)
    replacing_flush_new(->(*args) { b.freeze && Flush.__new(*args) }) do
      assert_raises(FrozenError) { b.push(1) }
    end
    assert_equal before, snapshot(b)
  end

  # `step` runs before the core: a controller that freezes the batch has
  # stepped (the binding cannot undo the caller's object), and retarget
  # raises FrozenError with the batch unchanged.
  def test_adapt_whose_step_freezes_the_batch_retargets_nothing
    b = Batch.new(10, 10)
    pid = ProportionalDuck.new(10.0)
    pid.define_singleton_method(:step) { |*args| b.freeze && super(*args) }
    assert_raises(FrozenError) { b.adapt(pid, 5.0, 1) }
    assert_equal 1, pid.steps
    assert_equal 10, b.target
  end

  def replacing_flush_new(replacement)
    Flush.singleton_class.alias_method(:__new, :new)
    Flush.define_singleton_method(:new, &replacement)
    yield
  ensure
    Flush.singleton_class.remove_method(:new)
    Flush.singleton_class.alias_method(:new, :__new)
    Flush.singleton_class.remove_method(:__new)
  end

  def test_an_empty_flush_records_no_clock
    b = Batch.new(2, 10)
    assert_equal 0, b.flush(9)
    assert_equal :buffered, b.push(4)
    assert_equal 1, b.flush(6)
    assert_equal 0, b.flush(20)
    assert_nil b.poll(6)
    assert_equal :time_regression, b.flush(5)
  end

  def test_strict_inputs
    assert_raises(RangeError) { Batch.new(0, 1) }
    assert_raises(RangeError) { Batch.new(2**32, 1) }
    assert_raises(RangeError) { Batch.new(1, 0) }
    assert_raises(ArgumentError) { Batch.new(1) }
    assert_raises(TypeError) { Batch.allocate }
    b = Batch.new(3, 10)
    [1.2, Float::NAN, '1', nil].each do |bad|
      assert_raises(TypeError) { b.push(bad) }
      assert_raises(TypeError) { b.push_weighted(0, bad) }
      assert_raises(TypeError) { b.poll(bad) }
      assert_raises(TypeError) { b.flush(bad) }
      assert_raises(TypeError) { Batch.new(bad, 10) }
      assert_raises(TypeError) { Batch.new(10, bad) }
    end
    [-1, 2**64].each { |bad| assert_raises(RangeError) { b.push(bad) } }
    [-1, 0, 2**32].each { |bad| assert_raises(RangeError) { b.push_weighted(0, bad) } }
    assert_equal 0, b.pending
  end

  def test_freeze
    b = Batch.new(1, 10).freeze
    pid = ProportionalDuck.new(1.0)
    assert_raises(FrozenError) { b.push(0) }
    assert_raises(FrozenError) { b.push_weighted(0, 1) }
    assert_raises(FrozenError) { b.poll(0) }
    assert_raises(FrozenError) { b.flush(0) }
    assert_raises(FrozenError) { b.adapt(pid, 1, 1) }
    assert_raises(FrozenError) { b.retarget(1) }
    # Frozen is checked before the arguments are read.
    [nil, -1, 1.5].each do |bad|
      assert_raises(FrozenError) { b.push(bad) }
      assert_raises(FrozenError) { b.push_weighted(0, bad) }
      assert_raises(FrozenError) { b.poll(bad) }
      assert_raises(FrozenError) { b.flush(bad) }
      assert_raises(FrozenError) { b.adapt(pid, 1, bad) }
    end
    assert_raises(FrozenError) { b.retarget('2') }
    assert_raises(FrozenError) { b.retarget(nil) }
    assert_equal 0, pid.steps
    assert_equal [1, 10, 0, 0, 1, nil], snapshot(b)
  end

  def test_the_extension_alone_is_a_name_error
    lib = File.expand_path('../lib', __dir__)
    script = 'begin; require "batch_machines_native/batch_machines_native"; ' \
             'rescue NameError => e; print e.class; end'
    output = IO.popen([RbConfig.ruby, '-I', lib, '-e', script], err: %i[child out], &:read)
    assert_equal 'NameError', output
  end

  def test_gc
    b = Batch.new(3, 10)
    b.push(1)
    GC.start
    GC.compact
    assert_equal SIZE, b.push_weighted(2, 2)
  end
end
