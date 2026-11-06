# frozen_string_literal: true
require 'minitest/autorun'
require 'batch_machines'

class BatchMachinesTest < Minitest::Test
  def test_size_full_flush_and_deadline
    b = BatchMachines::Batch.new(2, 10)
    assert_nil b.poll(0)
    assert_equal 'buffered', b.push(1)
    assert_equal 'size', b.push(2)
    assert_equal 'full', b.push(3)
    assert_equal 2, b.pending
    assert_equal 2, b.flush(3)
    assert_equal 0, b.flush(3)
    assert_equal 'buffered', b.push(4)
    assert_nil b.poll(13)
    assert_equal 'deadline', b.poll(14)
    assert_equal 'deadline', b.push(14) # accepted and deadline wins over size
    assert_equal 2, b.flush(14)
    assert_equal 0, b.pending
  end

  def test_pid_target_and_deadline_retained
    b = BatchMachines::Batch.new(5, 10)
    assert_equal 'buffered', b.push(0)
    b.configure_pid(1.0, 0.0, 0.0)
    assert_equal 2, b.adapt(2.9, 1_000_000_000)
    assert_equal 2, b.target
    assert_equal 'size', b.push(1)
    assert_equal 5, b.adapt(100.0, 1)
    assert_nil b.poll(9)
    assert_equal 'deadline', b.poll(10)
    assert_equal 1, b.adapt(-100.0, 1)
    assert_equal 'deadline', b.poll(10) # deadline still takes precedence
  end

  def test_pid_invalid_and_gc
    b = BatchMachines::Batch.new(3, 10)
    [Float::INFINITY, Float::NAN, -1.0].each do |bad|
      assert_raises(ArgumentError) { b.configure_pid(1, 0, bad) }
    end
    [Float::INFINITY, Float::NAN].each do |bad|
      assert_raises(ArgumentError) { b.adapt(bad, 1) }
    end
    assert_raises(ArgumentError) { b.adapt(1.0, 0) }
    assert_equal 3, b.target
    GC.start
    GC.compact
    assert_equal 2, b.adapt(2, 1_000_000_000)
    b.configure_pid(1.0e308, 0.0, 0.0)
    assert_nil b.adapt(1.0e308, 1_000_000_000)
    assert_equal 2, b.target
  end

  def test_strict_inputs_and_backwards_clock
    assert_raises(ArgumentError) { BatchMachines::Batch.new(0, 1) }
    assert_raises(ArgumentError) { BatchMachines::Batch.new(2**32, 1) }
    assert_raises(ArgumentError) { BatchMachines::Batch.new(1, 0) }
    b = BatchMachines::Batch.new(3, 10)
    [1.2, Float::NAN, Float::INFINITY, '1'].each do |bad|
      assert_raises(TypeError) { b.push(bad) }
      assert_raises(TypeError) { b.adapt(2.0, bad) }
      assert_raises(TypeError) { BatchMachines::Batch.new(bad, 10) }
    end
    [-1, 2**64].each { |bad| assert_raises(RangeError) { b.push(bad) } }
    assert_equal 0, b.pending
    assert_equal 'buffered', b.push(5)
    assert_equal 'rejected', b.push(4)
    assert_nil b.poll(4)
    assert_nil b.flush(4)
    assert_equal 1, b.pending
  end

  def test_freeze
    b = BatchMachines::Batch.new(1, 10).freeze
    assert_raises(FrozenError) { b.push(0) }
    assert_raises(FrozenError) { b.poll(0) }
    assert_raises(FrozenError) { b.flush(0) }
    assert_raises(FrozenError) { b.adapt(1, 1) }
    assert_raises(FrozenError) { b.configure_pid(1, 0, 0) }
    assert_equal 0, b.pending
  end
end
