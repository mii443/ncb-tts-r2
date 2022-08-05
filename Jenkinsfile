pipeline {
  agent any
  stages {
    stage('Build') {
      steps {
        echo "Build start."
      }

      post {
        always {
          echo "Build end."
        }
      }
    }
  }
}