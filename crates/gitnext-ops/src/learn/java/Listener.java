package gitnextlearn;

import org.junit.platform.engine.TestExecutionResult;
import org.junit.platform.engine.support.descriptor.ClassSource;
import org.junit.platform.engine.support.descriptor.MethodSource;
import org.junit.platform.launcher.TestExecutionListener;
import org.junit.platform.launcher.TestIdentifier;

/** Tells `Dump` when a test class and a test start and end. Found by the JUnit Platform through
 *  `META-INF/services`. */
public final class Listener implements TestExecutionListener {
    @Override
    public void executionStarted(TestIdentifier id) {
        if (id.isTest()) Dump.started(cls(id));
        else if (id.getSource().orElse(null) instanceof ClassSource) Dump.classStarted(((ClassSource) id.getSource().get()).getClassName());
    }

    static String cls(TestIdentifier id) {
        Object s = id.getSource().orElse(null);
        return s instanceof MethodSource ? ((MethodSource) s).getClassName() : "?";
    }

    @Override
    public void executionFinished(TestIdentifier id, TestExecutionResult r) {
        if (!id.isTest()) {
            if (id.getSource().orElse(null) instanceof ClassSource) Dump.classPart(((ClassSource) id.getSource().get()).getClassName());
            return;
        }
        String cls = "?", m = id.getDisplayName();
        if (id.getSource().orElse(null) instanceof MethodSource) {
            MethodSource s = (MethodSource) id.getSource().get();
            cls = s.getClassName();
            m = s.getMethodName();
        }
        Dump.finished(cls, m, r.getStatus() != TestExecutionResult.Status.SUCCESSFUL);
    }
}
